use proc_macro::TokenStream;
use quote::quote;
use syn::{Fields, ItemStruct, Meta, Type, parse_macro_input};

/// Attribute macro to create a reusable component with builder pattern that automatically implements Widget
///
/// # Attributes on fields
/// - `#[prop]` - Standard prop, generates builder method accepting `impl IntoMaybeDyn<T>`
/// - `#[prop(default = "expr")]` - Prop with default value
/// - `#[prop(callback)]` - Generates callback accepting `impl Fn() + Send + Sync + 'static`
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
                #name: Option<std::sync::Arc<dyn Fn() + Send + Sync>>
            }
        } else if field.is_children {
            quote! {
                __children: std::sync::RwLock<::guido::widgets::ChildrenSource>
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
                __children: std::sync::RwLock::new(::guido::widgets::ChildrenSource::default())
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
                #vis fn #name<F: Fn() + Send + Sync + 'static>(mut self, f: F) -> Self {
                    self.#name = Some(std::sync::Arc::new(f));
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
                child.add_to_container(&mut *self.__children.write().unwrap());
                self
            }

            #vis fn children<I>(self, children: I) -> Self
            where
                I: ::guido::widgets::IntoChildren,
            {
                children.add_to_container(&mut *self.__children.write().unwrap());
                self
            }

            /// Take the children source (consumes the children)
            fn take_children(&self) -> ::guido::widgets::ChildrenSource {
                std::mem::take(&mut *self.__children.write().unwrap())
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
            __inner: std::sync::RwLock<Option<Box<dyn ::guido::widgets::Widget>>>,
            __owner_id: std::sync::atomic::AtomicUsize,
        }

        impl #struct_name {
            #vis fn new() -> Self {
                Self {
                    #(#field_inits,)*
                    __inner: std::sync::RwLock::new(None),
                    __owner_id: std::sync::atomic::AtomicUsize::new(0),
                }
            }

            #(#builder_methods)*

            #children_methods

            fn ensure_built(&self) {
                // Check if already built using a read lock first
                if self.__inner.read().unwrap().is_some() {
                    return;
                }
                // Acquire write lock and check again (double-checked locking)
                let mut inner = self.__inner.write().unwrap();
                if inner.is_some() {
                    return;
                }
                // Wrap render() in an owner scope for automatic cleanup
                let (widget, owner_id) = ::guido::reactive::__internal::with_owner(|| {
                    self.render()
                });
                // Store owner_id as usize + 1 (0 means no owner)
                self.__owner_id.store(owner_id + 1, std::sync::atomic::Ordering::Relaxed);
                *inner = Some(Box::new(widget));
            }
        }

        impl Drop for #struct_name {
            fn drop(&mut self) {
                // Dispose the owner and all its signals/effects/cleanups
                let stored = self.__owner_id.load(std::sync::atomic::Ordering::Relaxed);
                if stored > 0 {
                    ::guido::reactive::__internal::dispose_owner(stored - 1);
                }
            }
        }

        impl ::guido::widgets::Widget for #struct_name {
            fn register_children(&mut self, tree: &mut ::guido::tree::Tree, id: ::guido::tree::WidgetId) {
                self.ensure_built();
                self.__inner.write().unwrap().as_mut().unwrap().register_children(tree, id)
            }

            fn reconcile_children(&mut self, tree: &mut ::guido::tree::Tree, id: ::guido::tree::WidgetId) -> bool {
                self.ensure_built();
                self.__inner.write().unwrap().as_mut().unwrap().reconcile_children(tree, id)
            }

            fn layout(&mut self, tree: &mut ::guido::tree::Tree, id: ::guido::tree::WidgetId, constraints: ::guido::layout::Constraints) -> ::guido::layout::Size {
                self.ensure_built();
                self.__inner.write().unwrap().as_mut().unwrap().layout(tree, id, constraints)
            }

            fn paint(&self, tree: &::guido::tree::Tree, id: ::guido::tree::WidgetId, ctx: &mut ::guido::renderer::PaintContext) {
                self.ensure_built();
                self.__inner.read().unwrap().as_ref().unwrap().paint(tree, id, ctx)
            }

            fn event(&mut self, tree: &mut ::guido::tree::Tree, id: ::guido::tree::WidgetId, event: &::guido::widgets::Event) -> ::guido::widgets::EventResponse {
                self.ensure_built();
                self.__inner.write().unwrap().as_mut().unwrap().event(tree, id, event)
            }

            fn set_origin(&mut self, x: f32, y: f32) {
                self.ensure_built();
                self.__inner.write().unwrap().as_mut().unwrap().set_origin(x, y)
            }

            fn bounds(&self) -> ::guido::widgets::Rect {
                self.ensure_built();
                self.__inner.read().unwrap().as_ref().unwrap().bounds()
            }

            fn has_focus_descendant(&self, tree: &::guido::tree::Tree, focused_id: ::guido::tree::WidgetId) -> bool {
                self.ensure_built();
                self.__inner.read().unwrap().as_ref().unwrap().has_focus_descendant(tree, focused_id)
            }

            fn is_relayout_boundary(&self) -> bool {
                self.ensure_built();
                self.__inner.read().unwrap().as_ref().unwrap().is_relayout_boundary()
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
