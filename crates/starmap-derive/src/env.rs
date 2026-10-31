//! The expansion of `#[derive(Env)]`.

use proc_macro2::TokenStream;
use quote::{ToTokens as _, quote, quote_spanned};
use syn::spanned::Spanned as _;
use syn::{Data, DeriveInput, Fields, GenericArgument, LitStr, PathArguments, Type, TypePath};

/// The spec file when the struct names none, relative to the crate root.
const DEFAULT_SPEC: &str = "starmap.toml";

/// Writes the `Env` impl of `input`.
pub(crate) fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let ident = &input.ident;
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new(
            input.generics.span(),
            "starmap::Env derives only for a struct without generic parameters",
        ));
    }
    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => &fields.named,
            _ => return Err(not_named(ident)),
        },
        _ => return Err(not_named(ident)),
    };
    let spec = spec_path(input)?;
    let mut described = Vec::new();
    let mut reads = Vec::new();
    for field in fields {
        let name = variable_name(field)?;
        let (inner, presence, method) = match option_inner(&field.ty) {
            Some(inner) => (inner, quote!(Maybe), quote!(read)),
            None => (&field.ty, quote!(Always), quote!(require)),
        };
        let rust_type: String = inner
            .to_token_stream()
            .to_string()
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect();
        let description = quote_spanned! {inner.span()=>
            ::starmap::Field::new::<#inner>(#name, #rust_type, ::starmap::Presence::#presence)
        };
        let member = &field.ident;
        reads.push(quote! {
            #member: #description.#method::<#inner>(environment)?
        });
        described.push(description);
    }
    Ok(quote! {
        impl ::starmap::Env for #ident {
            const SPEC: &'static str = ::core::include_str!(::core::concat!(
                ::core::env!("CARGO_MANIFEST_DIR"),
                "/",
                #spec
            ));
            const FIELDS: &'static [::starmap::Field] = &[#(#described),*];

            // rustc does not report an unfulfilled expectation in an external
            // macro's output, so an app whose clippy.toml allows vars_os builds clean.
            #[expect(
                clippy::disallowed_methods,
                reason = "BASE-6: Env::load is the one place an application reads its environment"
            )]
            fn load() -> ::core::result::Result<Self, ::starmap::ReadError> {
                <Self as ::starmap::Env>::from_vars(::std::env::vars_os())
            }

            fn from_environment(
                environment: &::starmap::Environment,
            ) -> ::core::result::Result<Self, ::starmap::FieldError> {
                // Method syntax lets rustc report an unsupported field type
                // once, at Field::new, and not again at the read.
                use ::starmap::__private::ReadField as _;
                ::core::result::Result::Ok(Self { #(#reads),* })
            }
        }
    })
}

/// The error for a type that is not a struct with named fields.
fn not_named(ident: &syn::Ident) -> syn::Error {
    syn::Error::new(
        ident.span(),
        "starmap::Env derives only for a struct with named fields",
    )
}

/// Returns the spec path of `#[starmap(spec = "...")]` on the struct, or
/// the default.
fn spec_path(input: &DeriveInput) -> syn::Result<LitStr> {
    let mut spec = None;
    for attribute in input
        .attrs
        .iter()
        .filter(|attribute| attribute.path().is_ident("starmap"))
    {
        attribute.parse_nested_meta(|meta| {
            if meta.path.is_ident("spec") && spec.is_none() {
                spec = Some(meta.value()?.parse::<LitStr>()?);
                Ok(())
            } else {
                Err(meta.error("the struct takes one #[starmap(spec = \"path\")]"))
            }
        })?;
    }
    Ok(spec.unwrap_or_else(|| LitStr::new(DEFAULT_SPEC, input.ident.span())))
}

/// Returns the variable name of `#[starmap("NAME")]` on `field`.
fn variable_name(field: &syn::Field) -> syn::Result<LitStr> {
    let mut attributes = field
        .attrs
        .iter()
        .filter(|attribute| attribute.path().is_ident("starmap"));
    match (attributes.next(), attributes.next()) {
        (Some(attribute), None) => attribute.parse_args::<LitStr>(),
        _ => Err(syn::Error::new(
            field.span(),
            "each field names its variable with one #[starmap(\"NAME\")]",
        )),
    }
}

/// Returns `T` when `ty` is `Option<T>`.
fn option_inner(ty: &Type) -> Option<&Type> {
    let Type::Path(TypePath {
        qself: None, path, ..
    }) = ty
    else {
        return None;
    };
    let segment = path.segments.last()?;
    let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return None;
    };
    match (
        segment.ident == "Option",
        arguments.args.first(),
        arguments.args.len(),
    ) {
        (true, Some(GenericArgument::Type(inner)), 1) => Some(inner),
        _ => None,
    }
}
