extern crate proc_macro;

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{
    Attribute, Data, DeriveInput, Fields, Lit, Meta, NestedMeta, Variant, parse_macro_input,
};

/// Derive macro for implementing `From<Error>` for HTTPError and `TryFrom<HTTPError>` for Error
#[proc_macro_derive(
    HTTPErrorConversion,
    attributes(bad_request, not_found, already_exists, failed_precondition)
)]
pub fn derive_http_error(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    // Extract the enum name and variants
    let enum_name = &input.ident;
    let variants = match &input.data {
        Data::Enum(data_enum) => &data_enum.variants,
        _ => panic!("HTTPErrorConversion can only be derived for enums"),
    };

    // Generate data structures for named fields and multiple unnamed fields
    let data_structs = generate_data_structs(variants, enum_name);

    // Generate the From<Error> implementation
    let match_arms_to_http = variants.iter().map(|variant| {
        let variant_name = &variant.ident;

        // Find the HTTP error attribute
        let http_error_attr = find_http_error_attr(variant);

        if let Some((error_code, error_code_str)) = http_error_attr {
            // Handle different field types
            match &variant.fields {
                Fields::Unit => {
                    // Unit variant (no fields)
                    quote! {
                        #enum_name::#variant_name => HTTPError::new(
                            #error_code,
                            #error_code_str,
                            Some(err.into()),
                            None::<()>,
                        ),
                    }
                }
                Fields::Unnamed(fields) => {
                    if fields.unnamed.len() == 1 {
                        // Single unnamed field - pass directly as before
                        quote! {
                            #enum_name::#variant_name(ref data) => {
                                let data_clone = data.clone();
                                HTTPError::new(
                                    #error_code,
                                    #error_code_str,
                                    Some(err.into()),
                                    Some(data_clone),
                                )
                            },
                        }
                    } else {
                        // Multiple unnamed fields - create tuple struct
                        let data_struct_name = format_ident!("{}Data", variant_name);
                        let field_names: Vec<_> = (0..fields.unnamed.len())
                            .map(|i| format_ident!("field_{}", i))
                            .collect();

                        quote! {
                            #enum_name::#variant_name(#(ref #field_names),*) => {
                                let data = #data_struct_name(#(#field_names.clone()),*);
                                HTTPError::new(
                                    #error_code,
                                    #error_code_str,
                                    Some(err.into()),
                                    Some(data),
                                )
                            },
                        }
                    }
                }
                Fields::Named(fields) => {
                    // Named fields - create struct with Data suffix
                    let data_struct_name = format_ident!("{}Data", variant_name);
                    let field_names: Vec<_> = fields
                        .named
                        .iter()
                        .map(|f| f.ident.as_ref().unwrap())
                        .collect();

                    quote! {
                        #enum_name::#variant_name { #(ref #field_names),* } => {
                            let data = #data_struct_name {
                                #(#field_names: #field_names.clone()),*
                            };
                            HTTPError::new(
                                #error_code,
                                #error_code_str,
                                Some(err.into()),
                                Some(data),
                            )
                        },
                    }
                }
            }
        } else {
            // No HTTP error attribute, use a default
            quote! {
                #enum_name::#variant_name { .. } => HTTPError::new(
                    ErrorCode::Internal,
                    "internal",
                    Some(err.into()),
                    None::<()>,
                ),
            }
        }
    });

    // Generate the TryFrom<HTTPError> implementation
    let match_arms_from_http = variants.iter().filter_map(|variant| {
        let variant_name = &variant.ident;

        // Find the HTTP error attribute
        let http_error_attr = find_http_error_attr(variant);

        if let Some((_, error_code_str)) = http_error_attr {
            // Handle different field types
            match &variant.fields {
                Fields::Unit => {
                    // Unit variant (no fields)
                    Some(quote! {
                        #error_code_str => Ok(#enum_name::#variant_name),
                    })
                }
                Fields::Unnamed(fields) => {
                    if fields.unnamed.len() == 1 {
                        // Single unnamed field - deserialize directly
                        Some(quote! {
                            #error_code_str => {
                                if let Some(data) = http_error.data {
                                    let data = serde_json::from_value(data)
                                        .map_err(|_| TryFromHTTPError::DeserializationError)?;
                                    Ok(#enum_name::#variant_name(data))
                                } else {
                                    Err(TryFromHTTPError::MissingData)
                                }
                            },
                        })
                    } else {
                        // Multiple unnamed fields - deserialize from tuple struct
                        let data_struct_name = format_ident!("{}Data", variant_name);
                        let field_bindings: Vec<_> = (0..fields.unnamed.len())
                            .map(|i| format_ident!("field_{}", i))
                            .collect();

                        Some(quote! {
                            #error_code_str => {
                                if let Some(data) = http_error.data {
                                    let #data_struct_name(#(#field_bindings),*) = serde_json::from_value(data)
                                        .map_err(|_| TryFromHTTPError::DeserializationError)?;
                                    Ok(#enum_name::#variant_name(#(#field_bindings),*))
                                } else {
                                    Err(TryFromHTTPError::MissingData)
                                }
                            },
                        })
                    }
                }
                Fields::Named(fields) => {
                    // Named fields - deserialize from generated struct
                    let data_struct_name = format_ident!("{}Data", variant_name);
                    let field_names: Vec<_> = fields.named.iter()
                        .map(|f| f.ident.as_ref().unwrap())
                        .collect();

                    Some(quote! {
                        #error_code_str => {
                            if let Some(data) = http_error.data {
                                let #data_struct_name { #(#field_names),* } = serde_json::from_value(data)
                                    .map_err(|_| TryFromHTTPError::DeserializationError)?;
                                Ok(#enum_name::#variant_name { #(#field_names),* })
                            } else {
                                Err(TryFromHTTPError::MissingData)
                            }
                        },
                    })
                }
            }
        } else {
            None
        }
    });

    // Add a derive(Clone) requirement comment to help users
    let output = quote! {
        #data_structs

        // Note: All data types used in tuple variants must implement Clone
        impl From<#enum_name> for HTTPError {
            fn from(err: #enum_name) -> Self {
                match err {
                    #(#match_arms_to_http)*
                }
            }
        }

        // Implement TryFrom<HTTPError> for Error
        impl std::convert::TryFrom<HTTPError> for #enum_name {
            type Error = TryFromHTTPError;

            fn try_from(http_error: HTTPError) -> Result<Self, Self::Error> {
                match http_error.reason.as_str() {
                    #(#match_arms_from_http)*
                    reason => Err(TryFromHTTPError::UnknownReason(reason.to_string())),
                }
            }
        }

        // Implement TryFrom<ErrorOutput> for Error
        impl std::convert::TryFrom<ErrorOutput> for #enum_name {
            type Error = TryFromHTTPError;

            fn try_from(error_output: ErrorOutput) -> Result<Self, Self::Error> {
                // Create an HTTPError from the ErrorOutput
                let http_error = HTTPError::new(
                    error_output.error.code,
                    &error_output.error.reason,
                    None,
                    error_output.error.data,
                );

                // Use the existing TryFrom<HTTPError> implementation
                Self::try_from(http_error)
            }
        }
    };

    output.into()
}

// Helper function to find and parse HTTP error attributes
fn find_http_error_attr(variant: &Variant) -> Option<(proc_macro2::TokenStream, String)> {
    for attr in &variant.attrs {
        if attr.path.is_ident("bad_request") {
            let error_code = quote! { ErrorCode::BadRequest };
            let error_code_str = extract_attr_string(attr);
            return Some((error_code, error_code_str));
        } else if attr.path.is_ident("not_found") {
            let error_code = quote! { ErrorCode::NotFound };
            let error_code_str = extract_attr_string(attr);
            return Some((error_code, error_code_str));
        } else if attr.path.is_ident("already_exists") {
            let error_code = quote! { ErrorCode::AlreadyExists };
            let error_code_str = extract_attr_string(attr);
            return Some((error_code, error_code_str));
        } else if attr.path.is_ident("failed_precondition") || attr.path.is_ident("internal") {
            let error_code = quote! { ErrorCode::FailedPrecondition };
            let error_code_str = extract_attr_string(attr);
            return Some((error_code, error_code_str));
        }
    }
    None
}

// Helper function to generate data structures for named and multiple unnamed fields
fn generate_data_structs(
    variants: &syn::punctuated::Punctuated<Variant, syn::token::Comma>,
    _enum_name: &syn::Ident,
) -> proc_macro2::TokenStream {
    let structs = variants.iter().filter_map(|variant| {
        let variant_name = &variant.ident;
        let data_struct_name = format_ident!("{}Data", variant_name);

        match &variant.fields {
            Fields::Named(fields) => {
                // Generate struct with named fields
                let field_definitions: Vec<_> = fields
                    .named
                    .iter()
                    .map(|f| {
                        let field_name = &f.ident;
                        let field_type = &f.ty;
                        let field_attrs = &f.attrs;

                        // Extract doc comments from field attributes
                        let doc_attrs: Vec<_> = field_attrs
                            .iter()
                            .filter(|attr| attr.path.is_ident("doc"))
                            .collect();

                        quote! {
                            #(#doc_attrs)*
                            pub #field_name: #field_type
                        }
                    })
                    .collect();

                let struct_doc = format!("Data structure for {variant_name} error variant");

                Some(quote! {
                    #[doc = #struct_doc]
                    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
                    pub struct #data_struct_name {
                        #(#field_definitions),*
                    }
                })
            }
            Fields::Unnamed(fields) if fields.unnamed.len() > 1 => {
                // Generate tuple struct for multiple unnamed fields
                let field_types: Vec<_> = fields.unnamed.iter().map(|f| &f.ty).collect();
                let struct_doc =
                    format!("Data structure for {variant_name} error variant (tuple fields)");

                Some(quote! {
                    #[doc = #struct_doc]
                    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
                    pub struct #data_struct_name(#(pub #field_types),*);
                })
            }
            _ => None,
        }
    });

    quote! {
        #(#structs)*
    }
}

// Helper function to extract string from attribute
fn extract_attr_string(attr: &Attribute) -> String {
    match attr.parse_meta() {
        Ok(Meta::List(meta_list)) => {
            if let Some(NestedMeta::Lit(Lit::Str(lit_str))) = meta_list.nested.first() {
                lit_str.value()
            } else {
                panic!("Expected string literal in attribute");
            }
        }
        _ => panic!("Expected attribute with string argument"),
    }
}
