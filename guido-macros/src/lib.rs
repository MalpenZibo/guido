use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{DeriveInput, Expr, Fields, ItemFn, Meta, Type, TypeBareFn, parse_macro_input};

/// Attribute macro to create a reusable component from a function.
///
/// The function name becomes the constructor (snake_case), and a PascalCase struct
/// is generated. Function parameters become props, and the function body becomes
/// the render method.
///
/// # Attributes on parameters
/// - No attribute — standard prop, `Signal<T>`, default = `create_stored(Default::default())`
/// - `#[prop(default = "expr")]` — standard prop with custom default
/// - `#[prop(callback)]` — callback prop. Use `()` for `Fn()`, or `fn(T1, T2)` for typed params
/// - `#[prop(children)]` — children support via `ChildrenSource`
/// - `#[prop(slot)]` — named widget slot
///
/// # Example
/// ```ignore
/// #[component]
/// pub fn button(
///     label: String,
///     #[prop(default = "Color::rgb(0.3, 0.3, 0.4)")]
///     background: Color,
///     #[prop(default = "8.0")]
///     padding: f32,
///     #[prop(callback)]
///     on_click: (),
/// ) -> impl Widget {
///     container()
///         .padding(padding) // Signal<f32> is Copy, no clone needed
///         .background(background) // Signal<Color> is Copy
///         .on_click_option(on_click.clone())
///         .child(text(label).color(Color::WHITE))
/// }
/// ```
#[proc_macro_attribute]
pub fn component(_attr: TokenStream, input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as ItemFn);

    let fn_name = &input.sig.ident;
    let vis = &input.vis;
    let body = &input.block;
    let struct_name = format_ident!("{}", to_pascal_case(&fn_name.to_string()));

    // Extract props from function parameters
    let mut prop_fields = Vec::new();
    let mut has_children = false;

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

        if let Some(prop_attr) = prop_attr
            && let Meta::List(meta_list) = &prop_attr.meta
        {
            let nested = meta_list
                .parse_args_with(
                    syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated,
                )
                .unwrap_or_default();

            for meta in &nested {
                if meta.path().is_ident("callback") {
                    is_callback = true;
                } else if meta.path().is_ident("children") {
                    is_children = true;
                    has_children = true;
                } else if meta.path().is_ident("slot") {
                    is_slot = true;
                } else if meta.path().is_ident("default") {
                    if let Meta::NameValue(nv) = meta {
                        // If the value is a string literal, parse its contents as an expression.
                        // This supports `#[prop(default = "Color::RED")]` syntax where the string
                        // contains arbitrary Rust expressions.
                        if let Expr::Lit(syn::ExprLit {
                            lit: syn::Lit::Str(lit_str),
                            ..
                        }) = &nv.value
                        {
                            match lit_str.parse::<Expr>() {
                                Ok(expr) => default_value = Some(expr),
                                Err(e) => {
                                    return syn::Error::new_spanned(
                                        lit_str,
                                        format!("failed to parse default value: {e}"),
                                    )
                                    .to_compile_error()
                                    .into();
                                }
                            }
                        } else {
                            default_value = Some(nv.value.clone());
                        }
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
            ty: field_type.clone(),
            default_value,
            is_callback,
            callback_params,
            is_children,
            is_slot,
        });
    }

    // Generate field definitions
    let field_defs = prop_fields.iter().map(|field| {
        let name = &field.name;
        let ty = &field.ty;

        if field.is_callback {
            let params = &field.callback_params;
            quote! {
                #name: Option<std::rc::Rc<dyn Fn(#(#params),*)>>
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

        if field.is_callback {
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
        let ty = &field.ty;

        if field.is_callback {
            let params = &field.callback_params;
            quote! {
                #vis fn #name<F: Fn(#(#params),*) + 'static>(mut self, f: F) -> Self {
                    self.#name = Some(std::rc::Rc::new(f));
                    self
                }
            }
        } else if field.is_children {
            // Don't generate a builder method for children - use child/children instead
            quote! {}
        } else if field.is_slot {
            quote! {
                #vis fn #name(self, widget: impl ::guido::widgets::Widget + 'static) -> Self {
                    *self.#name.borrow_mut() = Some(Box::new(widget));
                    self
                }
            }
        } else {
            quote! {
                #vis fn #name<__M>(mut self, value: impl ::guido::reactive::IntoSignal<#ty, __M>) -> Self {
                    self.#name = value.into_signal();
                    self
                }
            }
        }
    });

    // Generate children methods if needed
    let children_methods = if has_children {
        quote! {
            #vis fn child(self, child: impl ::guido::widgets::IntoChild) -> Self {
                child.add_to_container(&mut *self.__children.borrow_mut());
                self
            }

            #vis fn children<I>(self, children: I) -> Self
            where
                I: ::guido::widgets::IntoChildren,
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
            // Callbacks are Option<Rc<...>> — not Copy, need reference
            quote! {
                let #name = &self.#name;
            }
        } else {
            // Signal<T> is Copy — just copy it
            quote! {
                let #name = self.#name;
            }
        }
    });

    let expanded = quote! {
        #vis struct #struct_name {
            #(#field_defs,)*
            __inner: std::cell::RefCell<Option<Box<dyn ::guido::widgets::Widget>>>,
            __owner_id: std::cell::Cell<usize>,
        }

        impl #struct_name {
            #vis fn new() -> Self {
                Self {
                    #(#field_inits,)*
                    __inner: std::cell::RefCell::new(None),
                    __owner_id: std::cell::Cell::new(0),
                }
            }

            #(#builder_methods)*

            #children_methods

            #(#slot_methods)*

            fn render(&self) -> impl ::guido::widgets::Widget + use<> {
                #(#prop_bindings)*
                #body
            }

            fn ensure_built(&self) {
                if self.__inner.borrow().is_some() {
                    return;
                }
                // Wrap render() in an owner scope for automatic cleanup
                let (widget, owner_id) = ::guido::reactive::__internal::with_owner(|| {
                    self.render()
                });
                // Store owner_id + 1 (0 means no owner)
                self.__owner_id.set(owner_id + 1);
                *self.__inner.borrow_mut() = Some(Box::new(widget));
            }
        }

        impl Drop for #struct_name {
            fn drop(&mut self) {
                // Dispose the owner and all its signals/effects/cleanups
                let stored = self.__owner_id.get();
                if stored > 0 {
                    ::guido::reactive::__internal::dispose_owner(stored - 1);
                }
            }
        }

        impl ::guido::widgets::Widget for #struct_name {
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

            fn layout(&mut self, tree: &mut ::guido::tree::Tree, id: ::guido::tree::WidgetId, constraints: ::guido::layout::Constraints) -> ::guido::layout::Size {
                self.ensure_built();
                self.__inner.borrow_mut().as_mut().unwrap().layout(tree, id, constraints)
            }

            fn paint(&self, tree: &::guido::tree::Tree, id: ::guido::tree::WidgetId, ctx: &mut ::guido::renderer::PaintContext) {
                self.ensure_built();
                self.__inner.borrow().as_ref().unwrap().paint(tree, id, ctx)
            }

            fn event(&mut self, tree: &mut ::guido::tree::Tree, id: ::guido::tree::WidgetId, event: &::guido::widgets::Event) -> ::guido::widgets::EventResponse {
                self.ensure_built();
                self.__inner.borrow_mut().as_mut().unwrap().event(tree, id, event)
            }

            fn has_focus_descendant(&self, tree: &::guido::tree::Tree, focused_id: ::guido::tree::WidgetId) -> bool {
                self.ensure_built();
                self.__inner.borrow().as_ref().unwrap().has_focus_descendant(tree, focused_id)
            }
        }

        #vis fn #fn_name() -> #struct_name {
            #struct_name::new()
        }
    };

    TokenStream::from(expanded)
}

struct PropField {
    name: syn::Ident,
    ty: Type,
    default_value: Option<Expr>,
    is_callback: bool,
    /// For callbacks: parameter types extracted from `fn(T1, T2, ...)` field type.
    /// Empty vec means `Fn()` (unit type or no params).
    callback_params: Vec<Type>,
    is_children: bool,
    is_slot: bool,
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
/// #[derive(Clone, PartialEq, SignalFields)]
/// pub struct AppState {
///     pub count: i32,
///     pub name: String,
/// }
///
/// // Creates per-field signals from initial values
/// let state = AppStateSignals::new(AppState { count: 0, name: "foo".into() });
///
/// // Get writer handles for background tasks (Send + Copy)
/// let writers = state.writers();
///
/// // Widgets subscribe to individual signals
/// text(move || format!("Count: {}", state.count.get()))
/// ```
///
/// # Generic example
///
/// ```ignore
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
            quote! { pub #name: ::guido::reactive::signal::Signal<#ty> }
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
