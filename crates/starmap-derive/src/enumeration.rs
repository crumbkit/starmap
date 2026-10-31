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
    let mut values: Vec<LitStr> = Vec::new();
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
        let text = value.value();
        if text.is_empty() || text.chars().any(char::is_whitespace) {
            return Err(syn::Error::new(
                value.span(),
                "an enumeration value is not empty and holds no whitespace",
            ));
        }
        if values.iter().any(|earlier| earlier.value() == text) {
            return Err(syn::Error::new(
                value.span(),
                format!("two variants read as the value {text:?}, and each value must be distinct"),
            ));
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

        impl ::starmap::__private::FieldType for #ident {
            const SHAPE: ::starmap::Shape =
                ::starmap::Shape::Enumeration(<Self as ::starmap::Enumeration>::VALUES);
            const SHOW: ::starmap::Show = ::starmap::Show::Shown;

            fn read(
                environment: &::starmap::Environment,
                name: &::starmap::Name,
            ) -> ::core::result::Result<Self, ::starmap::FieldProblem> {
                ::starmap::__private::read_choice(environment, name)
            }
        }
    })
}

/// Returns `name` in kebab case, where a run of capitals is one word:
/// `DryRun` is `dry-run`, and `HTTPServer` is `http-server`.
fn kebab_case(name: &str) -> String {
    let characters: Vec<char> = name.chars().collect();
    let mut kebab = String::new();
    for (index, character) in characters.iter().enumerate() {
        let previous = index
            .checked_sub(1)
            .and_then(|before| characters.get(before));
        let next = characters.get(index + 1);
        let starts_word = character.is_uppercase()
            && previous.is_some_and(|previous| {
                !previous.is_uppercase() || next.is_some_and(|next| next.is_lowercase())
            });
        if starts_word {
            kebab.push('-');
        }
        kebab.extend(character.to_lowercase());
    }
    kebab
}
