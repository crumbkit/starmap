//! The expansion of `#[derive(Enumeration)]`.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, LitStr};

/// Writes the `Enumeration` impl of `input`.
pub(crate) fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let ident = &input.ident;
    let Data::Enum(data) = &input.data else {
        return Err(syn::Error::new(
            ident.span(),
            "starmap::Enumeration derives only for an enum of unit variants",
        ));
    };
    let mut values = Vec::new();
    let mut arms = Vec::new();
    for variant in &data.variants {
        if !matches!(variant.fields, Fields::Unit) {
            return Err(syn::Error::new(
                variant.ident.span(),
                "starmap::Enumeration derives only for an enum of unit variants",
            ));
        }
        let mut value = LitStr::new(
            &kebab_case(&variant.ident.to_string()),
            variant.ident.span(),
        );
        for attribute in variant
            .attrs
            .iter()
            .filter(|attribute| attribute.path().is_ident("starmap"))
        {
            value = attribute.parse_args::<LitStr>()?;
        }
        let variant = &variant.ident;
        arms.push(quote! { #value => ::core::option::Option::Some(Self::#variant) });
        values.push(value);
    }
    Ok(quote! {
        impl ::starmap::Enumeration for #ident {
            const VALUES: &'static [&'static str] = &[#(#values),*];

            fn from_choice(choice: &::starmap::Choice) -> ::core::option::Option<Self> {
                match choice.as_str() {
                    #(#arms,)*
                    _ => ::core::option::Option::None,
                }
            }
        }
    })
}

/// Returns `name` in kebab case: `DryRun` is `dry-run`.
fn kebab_case(name: &str) -> String {
    let mut kebab = String::new();
    for character in name.chars() {
        if character.is_uppercase() && !kebab.is_empty() {
            kebab.push('-');
        }
        kebab.extend(character.to_lowercase());
    }
    kebab
}
