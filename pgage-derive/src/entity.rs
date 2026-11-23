use proc_macro2::TokenStream;
use quote::quote;

pub(super) fn expand_entity(
    input: &syn::DeriveInput,
) -> syn::Result<TokenStream> {
    let attrs = EntityAttrs::new(input)?;
    let entity = attrs.impl_entity();
    let ty = attrs.impl_type();
    let encode = attrs.impl_encode();
    Ok(quote! {
        #entity
        #ty
        #encode
    })
}

struct EntityAttrs<'a> {
    name: syn::Ident,
    impl_gen: syn::ImplGenerics<'a>,
    ty_gen: syn::TypeGenerics<'a>,
    where_clause: Option<&'a syn::WhereClause>,
    label: LabelAttrs,
}

impl<'a> EntityAttrs<'a> {
    fn new(input: &'a syn::DeriveInput) -> syn::Result<Self> {
        let name = input.ident.clone();
        let label = match &input.data {
            syn::Data::Struct(_) => {
                LabelAttrs::One(label(&input.attrs, &input.ident)?)
            },
            syn::Data::Enum(data) => {
                if data.variants.is_empty() {
                    return Err(syn::Error::new_spanned(
                        &input.ident,
                        "`Entity` needs at least one variant",
                    ));
                }
                let mut variants = Vec::new();
                let mut labels = Vec::new();
                for v in &data.variants {
                    if !matches!(&v.fields, syn::Fields::Unnamed(f) if f.unnamed.len() == 1)
                    {
                        return Err(syn::Error::new_spanned(
                            v,
                            "`Entity` variants must have one unnamed field \
                             holding the properties",
                        ));
                    }
                    variants.push(v.ident.clone());
                    labels.push(label(&v.attrs, &v.ident)?);
                }
                LabelAttrs::Many { variants, labels }
            },
            syn::Data::Union(_) => {
                return Err(syn::Error::new_spanned(
                    &input.ident,
                    "`Entity` can't be derived for a union",
                ));
            },
        };
        let (impl_gen, ty_gen, where_clause) = input.generics.split_for_impl();
        Ok(Self { name, impl_gen, ty_gen, where_clause, label })
    }

    fn impl_entity(&self) -> TokenStream {
        let Self { name, impl_gen, ty_gen, where_clause, label } = self;
        let parse = label.parse();
        let to_parameter_map = label.to_parameter_map();
        quote! {
            impl #impl_gen ::pgage::cypher::Entity for #name #ty_gen #where_clause {
                fn parse(
                    label: &str,
                    properties: &str,
                ) -> ::core::result::Result<Self, ::pgage::AgError> {
                    #parse
                }

                fn to_parameter_map(
                    &self,
                ) -> ::core::result::Result<::std::string::String, ::pgage::AgError> {
                    #to_parameter_map
                }
            }
        }
    }

    fn impl_type(&self) -> TokenStream {
        let Self { name, impl_gen, ty_gen, where_clause, .. } = self;
        quote! {
            impl #impl_gen ::pgage::private::sqlx::Type<::pgage::private::sqlx::Postgres>
                for #name #ty_gen #where_clause
            {
                fn type_info() -> ::pgage::private::sqlx::postgres::PgTypeInfo {
                    ::pgage::private::sqlx::postgres::PgTypeInfo::with_name("agtype")
                }
            }
        }
    }

    fn impl_encode(&self) -> TokenStream {
        let Self { name, impl_gen, ty_gen, where_clause, .. } = self;
        quote! {
            impl #impl_gen ::pgage::private::sqlx::Encode<'_, ::pgage::private::sqlx::Postgres>
                for #name #ty_gen #where_clause
            {
                fn encode_by_ref(
                    &self,
                    buf: &mut ::pgage::private::sqlx::postgres::PgArgumentBuffer,
                ) -> ::core::result::Result<
                    ::pgage::private::sqlx::encode::IsNull,
                    ::pgage::private::sqlx::error::BoxDynError,
                > {
                    let map = ::pgage::cypher::Entity::to_parameter_map(self)?;
                    buf.push(::pgage::private::AGTYPE_VERSION);
                    buf.extend_from_slice(map.as_bytes());
                    ::core::result::Result::Ok(::pgage::private::sqlx::encode::IsNull::No)
                }
            }
        }
    }
}

enum LabelAttrs {
    One(syn::LitStr),
    Many { variants: Vec<syn::Ident>, labels: Vec<syn::LitStr> },
}

impl LabelAttrs {
    fn parse(&self) -> TokenStream {
        let unknown_label = quote! {
            <::pgage::AgError as ::pgage::private::serde::de::Error>::unknown_variant
        };
        match self {
            Self::One(label) => quote! {
                if label != #label {
                    return ::core::result::Result::Err(
                        #unknown_label(label, &[#label]),
                    );
                }
                ::pgage::private::serde_json::from_str(properties)
                    .map_err(::pgage::AgError::new)
            },
            Self::Many { variants, labels } => quote! {
                match label {
                    #(
                        #labels => ::pgage::private::serde_json::from_str(properties)
                            .map(Self::#variants)
                            .map_err(::pgage::AgError::new),
                    )*
                    _ => ::core::result::Result::Err(
                        #unknown_label(label, &[#(#labels),*]),
                    ),
                }
            },
        }
    }

    fn to_parameter_map(&self) -> TokenStream {
        match self {
            Self::One(_) => quote! {
                ::pgage::private::serde_json::to_string(self)
                    .map_err(::pgage::AgError::new)
            },
            Self::Many { variants, .. } => quote! {
                match self {
                    #(Self::#variants(p) => ::pgage::private::serde_json::to_string(p),)*
                }
                .map_err(::pgage::AgError::new)
            },
        }
    }
}

fn label(
    attrs: &[syn::Attribute],
    ident: &syn::Ident,
) -> syn::Result<syn::LitStr> {
    let mut label = None;
    for attr in attrs.iter().filter(|a| a.path().is_ident("pgage")) {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("label") {
                label = Some(meta.value()?.parse::<syn::LitStr>()?);
                Ok(())
            } else {
                Err(meta.error("unsupported `pgage` attribute"))
            }
        })?;
    }
    Ok(label
        .unwrap_or_else(|| syn::LitStr::new(&ident.to_string(), ident.span())))
}
