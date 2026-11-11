//! The expansion of `#[derive(Env)]`.

use proc_macro2::TokenStream;
use quote::{ToTokens as _, quote, quote_spanned};
use syn::parse::{Parse, ParseStream};
use syn::spanned::Spanned as _;
use syn::{
    Data, DeriveInput, Fields, GenericArgument, LitStr, Path, PathArguments, Token, Type, TypePath,
};

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
    let root = crate::pointer::crate_root();
    let followed = crate::pointer::follow(&spec.value(), |path| {
        root.as_deref()
            .and_then(|root| crate::pointer::read(root, path))
    })
    .map_err(|message| syn::Error::new(spec.span(), message))?;
    let embedded = included(&followed.file);
    let file = &followed.file;
    // The pointer's text is not embedded, and including it makes Cargo
    // rebuild the crate when the pointer changes.
    let tracked = followed.pointer.as_deref().map(|pointer| {
        let pointer = included(pointer);
        quote! { const _: &str = ::core::include_str!(#pointer); }
    });
    let mut described = Vec::new();
    let mut reads = Vec::new();
    let mut rules = Vec::new();
    for field in fields {
        let FieldAttribute { name, check } = field_attribute(field)?;
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
        if let Some(check) = check {
            // The check's path is spanned at the attribute, so rustc
            // names it when the function's signature does not fit.
            let check = quote_spanned! {check.span()=>
                ::starmap::__private::run_check::<#inner>(environment, name, #check)
            };
            rules.push(quote! {
                ::starmap::__private::Rule::new(#name, |environment, name| #check)
            });
        }
        described.push(description);
    }
    Ok(quote! {
        #tracked

        impl ::starmap::Env for #ident {
            const SPEC: &'static str = ::core::include_str!(#embedded);
            const SPEC_FILE: &'static str = #file;
            const FIELDS: &'static [::starmap::Field] = &[#(#described),*];
            const RULES: &'static [::starmap::__private::Rule] = &[#(#rules),*];

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

/// Returns the path expression that `include_str!` reads for `file`, which
/// is relative to the crate root unless it is absolute.
fn included(file: &str) -> TokenStream {
    if std::path::Path::new(file).is_absolute() {
        quote!(#file)
    } else {
        quote!(::core::concat!(::core::env!("CARGO_MANIFEST_DIR"), "/", #file))
    }
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

/// What a field's `#[starmap(...)]` attribute says: the variable's name,
/// and the path of the application's check, when it names one.
struct FieldAttribute {
    /// The name of the variable the field reads.
    name: LitStr,
    /// The function that `check = <path>` names.
    check: Option<Path>,
}

/// The message for a field attribute that does not parse.
const FIELD_ATTRIBUTE: &str = "each field names its variable with one #[starmap(\"NAME\")] or #[starmap(\"NAME\", check = path)]";

impl Parse for FieldAttribute {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let name = input.parse::<LitStr>()?;
        let mut check = None;
        if input.parse::<Option<Token![,]>>()?.is_some() && !input.is_empty() {
            let key = input.parse::<syn::Ident>()?;
            if key != "check" {
                return Err(syn::Error::new(key.span(), FIELD_ATTRIBUTE));
            }
            input.parse::<Token![=]>()?;
            check = Some(input.parse::<Path>()?);
            input.parse::<Option<Token![,]>>()?;
        }
        if !input.is_empty() {
            return Err(input.error(FIELD_ATTRIBUTE));
        }
        Ok(Self { name, check })
    }
}

/// Returns what the one `#[starmap(...)]` attribute on `field` says.
fn field_attribute(field: &syn::Field) -> syn::Result<FieldAttribute> {
    let mut attributes = field
        .attrs
        .iter()
        .filter(|attribute| attribute.path().is_ident("starmap"));
    match (attributes.next(), attributes.next()) {
        (Some(attribute), None) => attribute.parse_args::<FieldAttribute>(),
        _ => Err(syn::Error::new(field.span(), FIELD_ATTRIBUTE)),
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
