use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{DeriveInput, Fields, ItemStruct, Meta, Type, parse_macro_input};

/// Attribute macro to create a reusable component with builder pattern that automatically implements Widget
///
/// # Attributes on fields
/// - `#[prop]` - Standard prop, generates builder method accepting `impl IntoMaybeDyn<T>`
/// - `#[prop(default = "expr")]` - Prop with default value
/// - `#[prop(callback)]` - Generates callback accepting `impl Fn() + 'static`
/// - `#[prop(children)]` - Marks field for children support
///
/// # Example
/// ```ignore
/// #[component]
/// pub struct Button {
///     #[prop]
///     label: String,
///     #[prop(callback)]
///     on_click: (),
/// }
///
/// impl Button {
///     fn render(&self) -> impl Widget {
///         container().child(text(self.label.clone()))
///     }
/// }
/// ```
#[proc_macro_attribute]
pub fn component(_attr: TokenStream, input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as ItemStruct);

    let struct_name = &input.ident;
    let vis = &input.vis;

    // Extract fields
    let fields = match &input.fields {
        Fields::Named(fields) => &fields.named,
        _ => panic!("Component can only be used on structs with named fields"),
    };

    // Parse field information
    let mut prop_fields = Vec::new();
    let mut has_children = false;

    for field in fields {
        let field_name = field.ident.as_ref().unwrap();
        let field_type = &field.ty;

        // Skip if no #[prop] attribute
        let prop_attr = field.attrs.iter().find(|attr| attr.path().is_ident("prop"));

        if prop_attr.is_none() {
            continue;
        }

        let prop_attr = prop_attr.unwrap();

        // Parse attribute arguments
        let mut default_value: Option<String> = None;
        let mut is_callback = false;
        let mut is_children = false;

        if let Meta::List(meta_list) = &prop_attr.meta {
            let tokens_str = meta_list.tokens.to_string();

            if tokens_str.contains("callback") {
                is_callback = true;
            }

            if tokens_str.contains("children") {
                is_children = true;
                has_children = true;
            }

            if tokens_str.contains("default") {
                // Extract default value between quotes
                if let Some(start) = tokens_str.find('"')
                    && let Some(end) = tokens_str[start + 1..].find('"')
                {
                    default_value = Some(tokens_str[start + 1..start + 1 + end].to_string());
                }
            }
        }

        prop_fields.push(PropField {
            name: field_name.clone(),
            ty: field_type.clone(),
            default_value,
            is_callback,
            is_children,
        });
    }

    // Generate field definitions
    let field_defs = prop_fields.iter().map(|field| {
        let name = &field.name;
        let ty = &field.ty;

        if field.is_callback {
            quote! {
                #name: Option<std::rc::Rc<dyn Fn()>>
            }
        } else if field.is_children {
            quote! {
                __children: std::cell::RefCell<::guido::widgets::ChildrenSource>
            }
        } else {
            quote! {
                #name: ::guido::reactive::MaybeDyn<#ty>
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
        } else if let Some(default) = &field.default_value {
            let default_tokens: proc_macro2::TokenStream = default.parse().unwrap();
            quote! {
                #name: ::guido::reactive::MaybeDyn::Static(#default_tokens)
            }
        } else {
            quote! {
                #name: ::guido::reactive::MaybeDyn::Static(Default::default())
            }
        }
    });

    // Generate builder methods
    let builder_methods = prop_fields.iter().map(|field| {
        let name = &field.name;
        let ty = &field.ty;

        if field.is_callback {
            quote! {
                #vis fn #name<F: Fn() + 'static>(mut self, f: F) -> Self {
                    self.#name = Some(std::rc::Rc::new(f));
                    self
                }
            }
        } else if field.is_children {
            // Don't generate a builder method for children - use child/children instead
            quote! {}
        } else {
            quote! {
                #vis fn #name(mut self, value: impl ::guido::reactive::IntoMaybeDyn<#ty>) -> Self {
                    self.#name = value.into_maybe_dyn();
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

    // Create snake_case constructor name
    let constructor_name =
        syn::Ident::new(&to_snake_case(&struct_name.to_string()), struct_name.span());

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
            fn register_children(&mut self, tree: &mut ::guido::tree::Tree, id: ::guido::tree::WidgetId) {
                self.ensure_built();
                self.__inner.borrow_mut().as_mut().unwrap().register_children(tree, id)
            }

            fn reconcile_children(&mut self, tree: &mut ::guido::tree::Tree, id: ::guido::tree::WidgetId) -> bool {
                self.ensure_built();
                self.__inner.borrow_mut().as_mut().unwrap().reconcile_children(tree, id)
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

        #vis fn #constructor_name() -> #struct_name {
            #struct_name::new()
        }
    };

    TokenStream::from(expanded)
}

struct PropField {
    name: syn::Ident,
    ty: Type,
    default_value: Option<String>,
    is_callback: bool,
    is_children: bool,
}

fn to_snake_case(s: &str) -> String {
    let mut result = String::new();
    let mut prev_is_lower = false;

    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 && prev_is_lower {
                result.push('_');
            }
            result.push(c.to_lowercase().next().unwrap());
            prev_is_lower = false;
        } else {
            result.push(c);
            prev_is_lower = c.is_lowercase();
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
#[proc_macro_derive(SignalFields)]
pub fn derive_signal_fields(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let struct_name = &input.ident;
    let vis = &input.vis;

    let fields = match &input.data {
        syn::Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => &fields.named,
            _ => panic!("SignalFields can only be derived for structs with named fields"),
        },
        _ => panic!("SignalFields can only be derived for structs"),
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
        #[derive(Clone, Copy)]
        #vis struct #signals_name {
            #(#signals_fields,)*
        }

        impl #signals_name {
            pub fn new(initial: #struct_name) -> Self {
                Self {
                    #(#new_inits,)*
                }
            }

            pub fn writers(&self) -> #writers_name {
                #writers_name {
                    #(#writers_inits,)*
                }
            }
        }

        #[derive(Clone, Copy)]
        #vis struct #writers_name {
            #(#writers_fields,)*
        }

        impl #writers_name {
            pub fn set(&self, state: #struct_name) {
                #(#set_calls)*
            }
        }
    };

    TokenStream::from(expanded)
}
