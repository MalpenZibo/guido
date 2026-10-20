use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{
    Attribute, DeriveInput, Expr, Fields, ItemFn, Meta, Type, TypeBareFn, parse_macro_input,
};

/// Attribute macro to create a reusable component from a function.
///
/// The function name becomes the constructor (snake_case), and a PascalCase struct
/// is generated. Function parameters become props, and the function body becomes
/// the render method.
///
/// # Attributes on parameters
/// - No attribute — standard prop, `Signal<T>`, default = `create_stored(Default::default())`
/// - `#[prop(default = <expr>)]` — standard prop with custom default
/// - `#[prop(callback)]` — callback prop. Use `()` for `Fn()`, or `fn(T1, T2)` for typed params.
///   Its builder takes a closure, a `Callback` or an `Option<Callback>` (`IntoHandler`)
/// - `#[prop(children)]` — children support via `ChildrenSource`
/// - `#[prop(slot)]` — named widget slot
/// - `#[prop(required)]` — an argument of the constructor rather than a setter,
///   in declaration order: `icon(source).size(16.0)`. It takes what the setter
///   would have, a value, a closure or a signal, or a handler for a required
///   callback.
///   It cannot have a default, and a children or slot prop cannot be required.
///
/// A `///` on a parameter documents the method a caller sets that prop with —
/// `child` and `children` for the children prop. Rust refuses a doc comment on
/// a parameter only after expansion, and the macro never emits the parameters
/// again. The function's own `///` documents the struct and the constructor.
///
/// The function's type and const parameters, their bounds and its `where`
/// clause are carried onto the struct, its impls and the constructor, so a
/// component can be generic over its props' types. Lifetime parameters are
/// refused: a component is boxed as a `'static` widget. The generated methods'
/// own generics start with `__`, so they never shadow a name the caller chose.
///
/// # Example
/// ```ignore
/// // not compiled: `guido-macros` is a proc-macro crate and does not depend on
/// // `guido`, so a sample using `container()` has nothing to compile against.
/// // `tests/forwarded_children.rs` and `examples/component_example.rs` are the
/// // compiled versions.
/// #[component]
/// pub fn button(
///     label: String,
///     #[prop(default = Color::rgb(0.3, 0.3, 0.4))]
///     background: Color,
///     #[prop(default = 8.0)]
///     padding: f32,
///     #[prop(callback)]
///     on_click: (),
/// ) -> impl Widget {
///     container()
///         .padding(padding) // Signal<f32> is Copy, no clone needed
///         .background(background) // Signal<Color> is Copy
///         .on_click(on_click)
///         .child(text(label).color(Color::WHITE))
/// }
/// ```
/// The argument tuple for a callback prop: `()`, `(A,)`, `(A, B)`, …
fn callback_args_tuple(params: &[Type]) -> proc_macro2::TokenStream {
    match params.len() {
        0 => quote! { () },
        1 => {
            let only = &params[0];
            quote! { (#only,) }
        }
        _ => quote! { (#(#params),*) },
    }
}

#[proc_macro_attribute]
pub fn component(_attr: TokenStream, input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as ItemFn);

    let fn_name = &input.sig.ident;
    let vis = &input.vis;
    let fn_docs = doc_attrs(&input.attrs);
    let body = &input.block;
    let struct_name = format_ident!("{}", to_pascal_case(&fn_name.to_string()));

    let generics = &input.sig.generics;
    if let Some(lifetime) = generics.lifetimes().next() {
        return syn::Error::new_spanned(
            lifetime,
            "a component cannot take a lifetime parameter: it is boxed as a `'static` widget",
        )
        .to_compile_error()
        .into();
    }
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let type_params: Vec<_> = generics.type_params().map(|p| &p.ident).collect();
    // `render`'s `impl Widget` has to name every type and const parameter in
    // its `use<..>`; a lifetime is the only thing it may leave out.
    let captured = type_params
        .iter()
        .copied()
        .chain(generics.const_params().map(|p| &p.ident));

    // Extract props from function parameters
    let mut prop_fields = Vec::new();

    for arg in &input.sig.inputs {
        let syn::FnArg::Typed(pat_type) = arg else {
            return syn::Error::new_spanned(arg, "Component functions cannot have self parameters")
                .to_compile_error()
                .into();
        };

        // Get the parameter name
        let syn::Pat::Ident(pat_ident) = pat_type.pat.as_ref() else {
            return syn::Error::new_spanned(
                &pat_type.pat,
                "Component parameters must be simple identifiers",
            )
            .to_compile_error()
            .into();
        };
        let field_name = &pat_ident.ident;
        let field_type = &*pat_type.ty;

        // Check for #[prop(...)] attributes on the parameter
        let prop_attr = pat_type
            .attrs
            .iter()
            .find(|attr| attr.path().is_ident("prop"));

        let mut default_value: Option<Expr> = None;
        let mut is_callback = false;
        let mut is_children = false;
        let mut is_slot = false;
        let mut is_required = false;

        if let Some(prop_attr) = prop_attr
            && let Meta::List(meta_list) = &prop_attr.meta
        {
            // Reported, not swallowed. An unparseable list used to yield an
            // empty one, so a `#[prop(...)]` with a typo in it compiled as a
            // plain `Signal<T>` with `Default::default()` — taking `callback`,
            // `children` and `slot` on the same attribute down with it, and
            // leaving a component silently unwired instead of failing to build.
            // `default` now takes a raw expression, which is a great deal easier
            // to write wrong than the string it used to be.
            let nested = match meta_list.parse_args_with(
                syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated,
            ) {
                Ok(nested) => nested,
                Err(err) => return err.to_compile_error().into(),
            };

            for meta in &nested {
                if meta.path().is_ident("callback") {
                    is_callback = true;
                } else if meta.path().is_ident("children") {
                    is_children = true;
                } else if meta.path().is_ident("slot") {
                    is_slot = true;
                } else if meta.path().is_ident("required") {
                    is_required = true;
                } else if meta.path().is_ident("default") {
                    // The value is an ordinary Rust expression. It used to be
                    // accepted quoted as well, which made a string literal
                    // unspellable: `default = "Guest"` parsed its own contents
                    // and asked for a variable called Guest.
                    if let Meta::NameValue(nv) = meta {
                        default_value = Some(nv.value.clone());
                    } else {
                        return syn::Error::new_spanned(meta, "expected `default = <expr>`")
                            .to_compile_error()
                            .into();
                    }
                } else {
                    return syn::Error::new_spanned(
                        meta,
                        format!(
                            "unknown prop attribute: `{}`",
                            meta.path()
                                .get_ident()
                                .map_or_else(|| "?".to_string(), |i| i.to_string())
                        ),
                    )
                    .to_compile_error()
                    .into();
                }
            }

            // Validate: at most one of callback, children, slot
            let special_count = is_callback as u8 + is_children as u8 + is_slot as u8;
            if special_count > 1 {
                return syn::Error::new_spanned(
                    prop_attr,
                    "conflicting prop attributes: only one of `callback`, `children`, `slot` is allowed",
                )
                .to_compile_error()
                .into();
            }

            // Validate: callback, children, and slot props cannot have defaults
            if (is_callback || is_children || is_slot) && default_value.is_some() {
                return syn::Error::new_spanned(
                    prop_attr,
                    "callback, children, and slot props cannot have default values",
                )
                .to_compile_error()
                .into();
            }

            if is_required && default_value.is_some() {
                return syn::Error::new_spanned(
                    prop_attr,
                    "a required prop cannot have a default: the caller always gives it",
                )
                .to_compile_error()
                .into();
            }

            // A child is added and a slot filled by a method of its own; a
            // constructor argument for either would be a second spelling.
            if is_required && (is_children || is_slot) {
                return syn::Error::new_spanned(
                    prop_attr,
                    "children and slot props cannot be required",
                )
                .to_compile_error()
                .into();
            }
        }

        // For callback fields, extract parameter types from `fn(T1, T2, ...)` syntax
        let callback_params = if is_callback {
            if let Type::BareFn(TypeBareFn { inputs, .. }) = field_type {
                inputs.iter().map(|arg| arg.ty.clone()).collect()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        prop_fields.push(PropField {
            name: field_name.clone(),
            docs: doc_attrs(&pat_type.attrs),
            ty: field_type.clone(),
            default_value,
            is_callback,
            callback_params,
            is_children,
            is_slot,
            is_required,
        });
    }

    // Generate field definitions
    let field_defs = prop_fields.iter().map(|field| {
        let name = &field.name;
        let ty = &field.ty;

        if field.is_callback {
            let args = callback_args_tuple(&field.callback_params);
            quote! {
                #name: Option<::guido::reactive::Callback<#args>>
            }
        } else if field.is_children {
            quote! {
                __children: std::cell::RefCell<::guido::widgets::ChildrenSource>
            }
        } else if field.is_slot {
            quote! {
                #name: std::cell::RefCell<Option<Box<dyn ::guido::widgets::Widget>>>
            }
        } else {
            quote! {
                #name: ::guido::reactive::Signal<#ty>
            }
        }
    });

    // Generate field initializers for new()
    let field_inits = prop_fields.iter().map(|field| {
        let name = &field.name;

        if field.is_required {
            let value = field.convert(name);
            quote! {
                #name: #value
            }
        } else if field.is_callback {
            quote! {
                #name: None
            }
        } else if field.is_children {
            quote! {
                __children: std::cell::RefCell::new(::guido::widgets::ChildrenSource::default())
            }
        } else if field.is_slot {
            quote! {
                #name: std::cell::RefCell::new(None)
            }
        } else if let Some(default) = &field.default_value {
            quote! {
                #name: ::guido::reactive::create_stored(#default)
            }
        } else {
            quote! {
                #name: ::guido::reactive::create_stored(Default::default())
            }
        }
    });

    // Generate builder methods
    let builder_methods = prop_fields.iter().map(|field| {
        let name = &field.name;
        let docs = &field.docs;

        if field.is_required || field.is_children {
            // A required prop is given to the constructor, children to
            // `child` and `children`.
            quote! {}
        } else if field.is_slot {
            quote! {
                #(#docs)*
                #vis fn #name(self, widget: impl ::guido::widgets::Widget + 'static) -> Self {
                    *self.#name.borrow_mut() = Some(Box::new(widget));
                    self
                }
            }
        } else {
            let input = field.input(&format_ident!("__M"));
            let value = field.convert(&format_ident!("value"));
            quote! {
                #(#docs)*
                #vis fn #name<__M>(mut self, value: #input) -> Self {
                    self.#name = #value;
                    self
                }
            }
        }
    });

    // Generate children methods if needed
    let children_docs = prop_fields
        .iter()
        .find(|f| f.is_children)
        .map(|f| f.docs.as_slice());
    let children_methods = if let Some(docs) = children_docs {
        quote! {
            #(#docs)*
            #vis fn child<__M>(self, child: impl ::guido::widgets::IntoChild<__M>) -> Self {
                child.add_to_container(&mut *self.__children.borrow_mut());
                self
            }

            #(#docs)*
            #vis fn children<__M, __I>(self, children: __I) -> Self
            where
                __I: ::guido::widgets::IntoChildren<__M>,
            {
                children.add_to_container(&mut *self.__children.borrow_mut());
                self
            }

            /// Take the children source (consumes the children)
            fn take_children(&self) -> ::guido::widgets::ChildrenSource {
                std::mem::take(&mut *self.__children.borrow_mut())
            }
        }
    } else {
        quote! {}
    };

    // Generate take_ accessors for slot fields
    let slot_methods: Vec<_> = prop_fields
        .iter()
        .filter(|f| f.is_slot)
        .map(|field| {
            let name = &field.name;
            let take_name = format_ident!("take_{}", name);
            quote! {
                fn #take_name(&self) -> Option<Box<dyn ::guido::widgets::Widget>> {
                    self.#name.borrow_mut().take()
                }
            }
        })
        .collect();

    // Generate render method body: bind each prop as a local variable, then run the body
    let prop_bindings = prop_fields.iter().map(|field| {
        let name = &field.name;
        if field.is_children {
            quote! {
                let #name = self.take_children();
            }
        } else if field.is_slot {
            let take_name = format_ident!("take_{}", name);
            quote! {
                let #name = self.#take_name();
            }
        } else if field.is_callback {
            // Option<Callback<..>> is Copy: bind by value, no clone ceremony
            quote! {
                let #name = self.#name;
            }
        } else {
            // Signal<T> is Copy — just copy it
            quote! {
                let #name = self.#name;
            }
        }
    });

    // A required prop is an argument of `new` and of the constructor, in the
    // order it was declared, each under a marker generic of its own. Those
    // come after the component's own on the constructor, and are `new`'s alone
    // on the struct, so `Name::<P>::new(value)` names `P` and nothing else.
    let required: Vec<_> = prop_fields.iter().filter(|f| f.is_required).collect();
    let required_names: Vec<_> = required.iter().map(|f| &f.name).collect();
    let markers: Vec<_> = (0..required.len())
        .map(|i| format_ident!("__M{}", i))
        .collect();
    let required_params: Vec<_> = required
        .iter()
        .zip(&markers)
        .map(|(field, marker)| {
            let name = &field.name;
            let input = field.input(marker);
            quote! { #name: #input }
        })
        .collect();
    let mut fn_generics = generics.clone();
    fn_generics.params.extend(
        markers
            .iter()
            .map(|m| -> syn::GenericParam { syn::parse_quote!(#m) }),
    );
    let (fn_impl_generics, _, _) = fn_generics.split_for_impl();

    let expanded = quote! {
        #(#fn_docs)*
        #vis struct #struct_name #impl_generics #where_clause {
            #(#field_defs,)*
            __inner: std::cell::RefCell<Option<Box<dyn ::guido::widgets::Widget>>>,
            __owner_id: std::cell::Cell<Option<::guido::reactive::__internal::OwnerId>>,
            __made_in: Option<::guido::reactive::__internal::OwnerId>,
            // A type parameter only the body, a slot or the children use
            // appears in no other field, and a struct has to use each of its own.
            __generics: std::marker::PhantomData<fn() -> (#(#type_params,)*)>,
        }

        impl #impl_generics #struct_name #ty_generics #where_clause {
            /// Makes the component from its required props, with every
            /// other prop at its default.
            #vis fn new<#(#markers),*>(#(#required_params),*) -> Self {
                Self {
                    #(#field_inits,)*
                    __inner: std::cell::RefCell::new(None),
                    __owner_id: std::cell::Cell::new(None),
                    __made_in: ::guido::reactive::current_owner(),
                    __generics: std::marker::PhantomData,
                }
            }

            #(#builder_methods)*

            #children_methods

            #(#slot_methods)*

            fn render(&self) -> impl ::guido::widgets::Widget + use<#(#captured),*> {
                #(#prop_bindings)*
                #body
            }

            fn ensure_built(&self) {
                if self.__inner.borrow().is_some() {
                    return;
                }
                // The body runs when the tree first asks for something, which
                // is after the code that made the component returned. Its scope
                // hangs from the one current where it was made, so it reads
                // what that scope declared and goes when that scope goes.
                let (widget, owner_id) = ::guido::reactive::__internal::under_scope(
                    self.__made_in,
                    || ::guido::reactive::__internal::with_owner(|| self.render()),
                );
                self.__owner_id.set(Some(owner_id));
                *self.__inner.borrow_mut() = Some(Box::new(widget));
            }
        }

        impl #impl_generics Drop for #struct_name #ty_generics #where_clause {
            fn drop(&mut self) {
                // Dispose the owner and all its signals/effects/cleanups. If
                // the scope it was made in went first, this one went with it
                // and the id no longer names anything: disposing it is nothing.
                if let Some(owner_id) = self.__owner_id.get() {
                    ::guido::reactive::__internal::dispose_owner(owner_id);
                }
            }
        }

        impl #impl_generics ::guido::widgets::Widget for #struct_name #ty_generics #where_clause {
            fn advance_animations(&mut self, tree: &mut ::guido::tree::Tree, id: ::guido::tree::WidgetId) -> bool {
                self.ensure_built();
                self.__inner.borrow_mut().as_mut().unwrap().advance_animations(tree, id)
            }

            fn register_children(&mut self, tree: &mut ::guido::tree::Tree, id: ::guido::tree::WidgetId) {
                self.ensure_built();
                self.__inner.borrow_mut().as_mut().unwrap().register_children(tree, id)
            }

            fn reconcile_children(&mut self, tree: &mut ::guido::tree::Tree, id: ::guido::tree::WidgetId) -> bool {
                self.ensure_built();
                self.__inner.borrow_mut().as_mut().unwrap().reconcile_children(tree, id)
            }

            fn layout_hints(&self) -> ::guido::widgets::LayoutHints {
                self.ensure_built();
                self.__inner.borrow().as_ref()
                    .map(|w| w.layout_hints())
                    .unwrap_or_default()
            }

            fn layout(&mut self, ctx: &mut ::guido::tree::LayoutCtx, constraints: ::guido::layout::Constraints) -> ::guido::layout::Size {
                self.ensure_built();
                self.__inner.borrow_mut().as_mut().unwrap().layout(ctx, constraints)
            }

            fn paint(&self, ctx: &mut ::guido::renderer::PaintContext) {
                self.ensure_built();
                self.__inner.borrow().as_ref().unwrap().paint(ctx)
            }

            fn event(&mut self, tree: &mut ::guido::tree::Tree, id: ::guido::tree::WidgetId, event: &::guido::widgets::Event) -> ::guido::widgets::EventResponse {
                self.ensure_built();
                self.__inner.borrow_mut().as_mut().unwrap().event(tree, id, event)
            }

            fn begin_exit(&mut self, tree: &mut ::guido::tree::Tree, id: ::guido::tree::WidgetId) -> bool {
                self.ensure_built();
                self.__inner.borrow_mut().as_mut().unwrap().begin_exit(tree, id)
            }

            fn is_exiting(&self) -> bool {
                self.ensure_built();
                self.__inner.borrow().as_ref().unwrap().is_exiting()
            }

            fn cancel_exit(&mut self, tree: &mut ::guido::tree::Tree, id: ::guido::tree::WidgetId) {
                self.ensure_built();
                self.__inner.borrow_mut().as_mut().unwrap().cancel_exit(tree, id)
            }

            // The scope the body ran in, which the component owns, rather
            // than whatever the widget it built answers — as `OwnedWidget`
            // answers for the scope it made.
            fn owned_scope(&self) -> Option<::guido::reactive::__internal::OwnerId> {
                self.__owner_id.get()
            }

            fn refresh_paint_bounds(&self, tree: &mut ::guido::tree::Tree, id: ::guido::tree::WidgetId) {
                self.ensure_built();
                self.__inner.borrow().as_ref().unwrap().refresh_paint_bounds(tree, id)
            }

            fn placement(&self, tree: &::guido::tree::Tree, id: ::guido::tree::WidgetId) -> ::guido::widgets::Placement {
                self.ensure_built();
                self.__inner.borrow().as_ref().unwrap().placement(tree, id)
            }

        }

        #(#fn_docs)*
        #vis fn #fn_name #fn_impl_generics (#(#required_params),*) -> #struct_name #ty_generics #where_clause {
            #struct_name::new(#(#required_names),*)
        }
    };

    TokenStream::from(expanded)
}

struct PropField {
    name: syn::Ident,
    /// The parameter's `///`, for the method that sets it.
    docs: Vec<Attribute>,
    ty: Type,
    default_value: Option<Expr>,
    is_callback: bool,
    /// For callbacks: parameter types extracted from `fn(T1, T2, ...)` field type.
    /// Empty vec means `Fn()` (unit type or no params).
    callback_params: Vec<Type>,
    is_children: bool,
    is_slot: bool,
    /// Taken by the constructor rather than by a setter.
    is_required: bool,
}

impl PropField {
    /// What a setter or a required argument takes for this prop: a handler
    /// for a callback, anything that converts into its signal otherwise.
    fn input(&self, marker: &syn::Ident) -> proc_macro2::TokenStream {
        if self.is_callback {
            let args = callback_args_tuple(&self.callback_params);
            quote! { impl ::guido::reactive::IntoHandler<#args, #marker> }
        } else {
            let ty = &self.ty;
            quote! { impl ::guido::reactive::IntoSignal<#ty, #marker> }
        }
    }

    /// What the field holds, made from `value`, an argument of `input`'s type.
    fn convert(&self, value: &syn::Ident) -> proc_macro2::TokenStream {
        if self.is_callback {
            quote! { #value.into_callback_prop() }
        } else {
            quote! { #value.into_signal() }
        }
    }
}

fn doc_attrs(attrs: &[Attribute]) -> Vec<Attribute> {
    attrs
        .iter()
        .filter(|attr| attr.path().is_ident("doc"))
        .cloned()
        .collect()
}

fn to_pascal_case(s: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = true;

    for c in s.chars() {
        if c == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.push(c.to_uppercase().next().unwrap());
            capitalize_next = false;
        } else {
            result.push(c);
        }
    }

    result
}

/// Derive macro for per-field signal decomposition.
///
/// Generates two companion structs for a given struct:
/// - `{Name}Signals` — contains a `Signal<T>` for each field (`Copy`)
/// - `{Name}Writers` — contains a `WriteSignal<T>` for each field (`Copy + Send`)
///
/// Supports generic structs — the generated types carry the same generic parameters.
///
/// # Example
///
/// ```ignore
/// // not compiled: see the `component` sample above — this crate has nothing to
/// // compile a widget against.
/// #[derive(Clone, PartialEq, SignalFields)]
/// pub struct AppState {
///     pub count: i32,
///     pub name: String,
/// }
///
/// // Creates per-field signals from initial values
/// let state = AppStateSignals::new(AppState { count: 0, name: "foo".into() });
///
/// // Get writer handles for background tasks (Send + Copy): an AppStateWriters
/// let writers = state.writers();
///
/// // Widgets subscribe to individual signals
/// text(move || format!("Count: {}", state.count.get()))
/// ```
///
/// # Generic example
///
/// ```ignore
/// // not compiled: see the `component` sample above.
/// #[derive(Clone, PartialEq, SignalFields)]
/// pub struct Pair<A: Clone + PartialEq + Send + 'static, B: Clone + PartialEq + Send + 'static> {
///     pub first: A,
///     pub second: B,
/// }
///
/// let pair = PairSignals::new(Pair { first: 1i32, second: "hello".to_string() });
/// ```
#[proc_macro_derive(SignalFields)]
pub fn derive_signal_fields(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let struct_name = &input.ident;
    let vis = &input.vis;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let fields = match &input.data {
        syn::Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => &fields.named,
            _ => {
                return syn::Error::new_spanned(
                    &input,
                    "SignalFields can only be derived for structs with named fields",
                )
                .to_compile_error()
                .into();
            }
        },
        _ => {
            return syn::Error::new_spanned(&input, "SignalFields can only be derived for structs")
                .to_compile_error()
                .into();
        }
    };

    let signals_name = format_ident!("{}Signals", struct_name);
    let writers_name = format_ident!("{}Writers", struct_name);

    let field_names: Vec<_> = fields.iter().map(|f| f.ident.as_ref().unwrap()).collect();
    let field_types: Vec<_> = fields.iter().map(|f| &f.ty).collect();

    // Generate {Name}Signals struct fields
    let signals_fields = field_names
        .iter()
        .zip(field_types.iter())
        .map(|(name, ty)| {
            quote! { pub #name: ::guido::reactive::signal::RwSignal<#ty> }
        });

    // Generate {Name}Writers struct fields
    let writers_fields = field_names
        .iter()
        .zip(field_types.iter())
        .map(|(name, ty)| {
            quote! { pub #name: ::guido::reactive::signal::WriteSignal<#ty> }
        });

    // Generate new() field initializers: create_signal(initial.field)
    let new_inits = field_names.iter().map(|name| {
        quote! { #name: ::guido::reactive::signal::create_signal(initial.#name) }
    });

    // Generate writers() field initializers: self.field.writer()
    let writers_inits = field_names.iter().map(|name| {
        quote! { #name: self.#name.writer() }
    });

    // Generate set() calls: self.field.set(state.field)
    let set_calls = field_names.iter().map(|name| {
        quote! { self.#name.set(state.#name); }
    });

    let expanded = quote! {
        #vis struct #signals_name #impl_generics #where_clause {
            #(#signals_fields,)*
        }

        // Manual Clone/Copy — Signal<T> is Copy regardless of T,
        // but #[derive(Copy)] would add a spurious T: Copy bound.
        impl #impl_generics Clone for #signals_name #ty_generics #where_clause {
            fn clone(&self) -> Self { *self }
        }
        impl #impl_generics Copy for #signals_name #ty_generics #where_clause {}

        impl #impl_generics #signals_name #ty_generics #where_clause {
            pub fn new(initial: #struct_name #ty_generics) -> Self {
                Self {
                    #(#new_inits,)*
                }
            }

            pub fn writers(&self) -> #writers_name #ty_generics {
                #writers_name {
                    #(#writers_inits,)*
                }
            }
        }

        #vis struct #writers_name #impl_generics #where_clause {
            #(#writers_fields,)*
        }

        // Manual Clone/Copy — WriteSignal<T> is Copy regardless of T.
        impl #impl_generics Clone for #writers_name #ty_generics #where_clause {
            fn clone(&self) -> Self { *self }
        }
        impl #impl_generics Copy for #writers_name #ty_generics #where_clause {}

        impl #impl_generics #writers_name #ty_generics #where_clause {
            pub fn set(&self, state: #struct_name #ty_generics) {
                ::guido::reactive::__internal::batch(|| {
                    #(#set_calls)*
                });
            }
        }
    };

    TokenStream::from(expanded)
}
