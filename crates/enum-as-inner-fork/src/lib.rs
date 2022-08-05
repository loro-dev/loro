// Copyright 2015-2018 Benjamin Fry <benjaminfry@me.com>
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

//! # enum-as-inner
//!
//! A deriving proc-macro for generating functions to automatically give access to the inner members of enum.
//!
//! ## Basic unnamed field case
//!
//! The basic case is meant for single item enums, like:
//!
//! ```rust
//! use enum_as_inner::EnumAsInner;
//!
//! #[derive(Debug, EnumAsInner)]
//! enum OneEnum {
//!     One(u32),
//! }
//!
//! let one = OneEnum::One(1);
//!
//! assert_eq!(*one.as_one().unwrap(), 1);
//! assert_eq!(one.into_one().unwrap(), 1);
//! ```
//!
//! where the result is either a reference for inner items or a tuple containing the inner items.
//!
//! ## Unit case
//!
//! This will return true if enum's variant matches the expected type
//!
//! ```rust
//! use enum_as_inner::EnumAsInner;
//!
//! #[derive(EnumAsInner)]
//! enum UnitVariants {
//!     Zero,
//!     One,
//!     Two,
//! }
//!
//! let unit = UnitVariants::Two;
//!
//! assert!(unit.is_two());
//! ```
//!
//! ## Mutliple, unnamed field case
//!
//! This will return a tuple of the inner types:
//!
//! ```rust
//! use enum_as_inner::EnumAsInner;
//!
//! #[derive(Debug, EnumAsInner)]
//! enum ManyVariants {
//!     One(u32),
//!     Two(u32, i32),
//!     Three(bool, u32, i64),
//! }
//!
//! let many = ManyVariants::Three(true, 1, 2);
//!
//! assert_eq!(many.as_three().unwrap(), (&true, &1_u32, &2_i64));
//! assert_eq!(many.into_three().unwrap(), (true, 1_u32, 2_i64));
//! ```
//!
//! ## Multiple, named field case
//!
//! This will return a tuple of the inner types, like the unnamed option:
//!
//! ```rust
//! use enum_as_inner::EnumAsInner;
//!
//! #[derive(Debug, EnumAsInner)]
//! enum ManyVariants {
//!     One { one: u32 },
//!     Two { one: u32, two: i32 },
//!     Three { one: bool, two: u32, three: i64 },
//! }
//!
//! let many = ManyVariants::Three { one: true, two: 1, three: 2 };
//!
//! assert_eq!(many.as_three().unwrap(), (&true, &1_u32, &2_i64));
//! assert_eq!(many.into_three().unwrap(), (true, 1_u32, 2_i64));
//! ```

#![warn(
    clippy::default_trait_access,
    clippy::dbg_macro,
    clippy::print_stdout,
    clippy::unimplemented,
    clippy::use_self,
    missing_copy_implementations,
    missing_docs,
    non_snake_case,
    non_upper_case_globals,
    rust_2018_idioms,
    unreachable_pub
)]

use heck::ToSnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use syn::{parse_macro_input, DeriveInput};

/// returns first the types to return, the match names, and then tokens to the field accesses
fn unit_fields_return(variant_name: &syn::Ident, function_name: &Ident, doc: &str) -> TokenStream {
    quote!(
        #[doc = #doc]
        #[inline]
        pub fn #function_name(&self) -> bool {
            matches!(self, Self::#variant_name)
        }
    )
}

/// returns first the types to return, the match names, and then tokens to the field accesses
fn unnamed_fields_return(
    variant_name: &syn::Ident,
    (function_name_mut_ref, doc_mut_ref): (&Ident, &str),
    (function_name_ref, doc_ref): (&Ident, &str),
    (function_name_val, doc_val): (&Ident, &str),
    fields: &syn::FieldsUnnamed,
) -> TokenStream {
    let (returns_mut_ref, returns_ref, returns_val, matches) = match fields.unnamed.len() {
        1 => {
            let field = fields.unnamed.first().expect("no fields on type");

            let returns = &field.ty;
            let returns_mut_ref = quote!(&mut #returns);
            let returns_ref = quote!(&#returns);
            let returns_val = quote!(#returns);
            let matches = quote!(inner);

            (returns_mut_ref, returns_ref, returns_val, matches)
        }
        0 => (quote!(()), quote!(()), quote!(()), quote!()),
        _ => {
            let mut returns_mut_ref = TokenStream::new();
            let mut returns_ref = TokenStream::new();
            let mut returns_val = TokenStream::new();
            let mut matches = TokenStream::new();

            for (i, field) in fields.unnamed.iter().enumerate() {
                let rt = &field.ty;
                let match_name = Ident::new(&format!("match_{}", i), Span::call_site());
                returns_mut_ref.extend(quote!(&mut #rt,));
                returns_ref.extend(quote!(&#rt,));
                returns_val.extend(quote!(#rt,));
                matches.extend(quote!(#match_name,));
            }

            (
                quote!((#returns_mut_ref)),
                quote!((#returns_ref)),
                quote!((#returns_val)),
                quote!(#matches),
            )
        }
    };

    quote!(
        #[doc = #doc_mut_ref ]
        #[inline]
        pub fn #function_name_mut_ref(&mut self) -> Option<#returns_mut_ref> {
            match self {
                Self::#variant_name(#matches) => {
                    Some((#matches))
                }
                _ => None
            }
        }

        #[doc = #doc_ref ]
        #[inline]
        pub fn #function_name_ref(&self) -> Option<#returns_ref> {
            match self {
                Self::#variant_name(#matches) => {
                    Some((#matches))
                }
                _ => None
            }
        }

        #[doc = #doc_val ]
        #[inline]
        pub fn #function_name_val(self) -> ::core::result::Result<#returns_val, Self> {
            match self {
                Self::#variant_name(#matches) => {
                    Ok((#matches))
                },
                _ => Err(self)
            }
        }
    )
}

/// returns first the types to return, the match names, and then tokens to the field accesses
fn named_fields_return(
    variant_name: &syn::Ident,
    (function_name_mut_ref, doc_mut_ref): (&Ident, &str),
    (function_name_ref, doc_ref): (&Ident, &str),
    (function_name_val, doc_val): (&Ident, &str),
    fields: &syn::FieldsNamed,
) -> TokenStream {
    let (returns_mut_ref, returns_ref, returns_val, matches) = match fields.named.len() {
        1 => {
            let field = fields.named.first().expect("no fields on type");
            let match_name = field.ident.as_ref().expect("expected a named field");

            let returns = &field.ty;
            let returns_mut_ref = quote!(&mut #returns);
            let returns_ref = quote!(&#returns);
            let returns_val = quote!(#returns);
            let matches = quote!(#match_name);

            (returns_mut_ref, returns_ref, returns_val, matches)
        }
        0 => (quote!(()), quote!(()), quote!(()), quote!(())),
        _ => {
            let mut returns_mut_ref = TokenStream::new();
            let mut returns_ref = TokenStream::new();
            let mut returns_val = TokenStream::new();
            let mut matches = TokenStream::new();

            for field in fields.named.iter() {
                let rt = &field.ty;
                let match_name = field.ident.as_ref().expect("expected a named field");

                returns_mut_ref.extend(quote!(&mut #rt,));
                returns_ref.extend(quote!(&#rt,));
                returns_val.extend(quote!(#rt,));
                matches.extend(quote!(#match_name,));
            }

            (
                quote!((#returns_mut_ref)),
                quote!((#returns_ref)),
                quote!((#returns_val)),
                quote!(#matches),
            )
        }
    };

    quote!(
        #[doc = #doc_mut_ref ]
        #[inline]
        pub fn #function_name_mut_ref(&mut self) -> Option<#returns_mut_ref> {
            match self {
                Self::#variant_name{ #matches } => {
                    Some((#matches))
                }
                _ => None
            }
        }

        #[doc = #doc_ref ]
        #[inline]
        pub fn #function_name_ref(&self) -> Option<#returns_ref> {
            match self {
                Self::#variant_name{ #matches } => {
                    Some((#matches))
                }
                _ => None
            }
        }

        #[doc = #doc_val ]
        #[inline]
        pub fn #function_name_val(self) -> ::core::result::Result<#returns_val, Self> {
            match self {
                Self::#variant_name{ #matches } => {
                    Ok((#matches))
                }
                _ => Err(self)
            }
        }
    )
}

fn impl_all_as_fns(ast: &DeriveInput) -> TokenStream {
    let name = &ast.ident;
    let generics = &ast.generics;

    let enum_data = if let syn::Data::Enum(data) = &ast.data {
        data
    } else {
        panic!("{} is not an enum", name);
    };

    let mut stream = TokenStream::new();

    for variant_data in &enum_data.variants {
        let variant_name = &variant_data.ident;
        let function_name_ref = Ident::new(
            &format!("as_{}", variant_name).to_snake_case(),
            Span::call_site(),
        );
        let doc_ref = format!(
            "Optionally returns references to the inner fields if this is a `{}::{}`, otherwise `None`",
            name,
            variant_name,
        );
        let function_name_mut_ref = Ident::new(
            &format!("as_{}_mut", variant_name).to_snake_case(),
            Span::call_site(),
        );
        let doc_mut_ref = format!(
            "Optionally returns mutable references to the inner fields if this is a `{}::{}`, otherwise `None`",
            name,
            variant_name,
        );

        let function_name_val = Ident::new(
            &format!("into_{}", variant_name).to_snake_case(),
            Span::call_site(),
        );
        let doc_val = format!(
            "Returns the inner fields if this is a `{}::{}`, otherwise returns back the enum in the `Err` case of the result",
            name,
            variant_name,
        );

        let function_name_is = Ident::new(
            &format!("is_{}", variant_name).to_snake_case(),
            Span::call_site(),
        );
        let doc_is = format!(
            "Returns true if this is a `{}::{}`, otherwise false",
            name, variant_name,
        );

        let tokens = match &variant_data.fields {
            syn::Fields::Unit => unit_fields_return(variant_name, &function_name_is, &doc_is),
            syn::Fields::Unnamed(unnamed) => unnamed_fields_return(
                variant_name,
                (&function_name_mut_ref, &doc_mut_ref),
                (&function_name_ref, &doc_ref),
                (&function_name_val, &doc_val),
                unnamed,
            ),
            syn::Fields::Named(named) => named_fields_return(
                variant_name,
                (&function_name_mut_ref, &doc_mut_ref),
                (&function_name_ref, &doc_ref),
                (&function_name_val, &doc_val),
                named,
            ),
        };

        stream.extend(tokens);
    }

    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    quote!(
        impl #impl_generics #name #ty_generics #where_clause {
            #stream
        }
    )
}

/// Derive functions on an Enum for easily accessing individual items in the Enum
#[proc_macro_derive(EnumAsInner)]
pub fn enum_as_inner(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    // get a usable token stream
    let ast: DeriveInput = parse_macro_input!(input as DeriveInput);

    // Build the impl
    let expanded: TokenStream = impl_all_as_fns(&ast);

    // Return the generated impl
    proc_macro::TokenStream::from(expanded)
}
