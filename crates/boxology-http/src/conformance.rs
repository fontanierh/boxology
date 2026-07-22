use boxology_contract::{
    CapabilityDescriptor, CapabilityShape, DescriptorRef, Detail, TypeDescriptor, VariantPayload,
};

pub(crate) fn conform_capability(descriptor: &CapabilityDescriptor) -> Result<(), Detail> {
    if descriptor.shape() != CapabilityShape::Unary {
        return Err(
            Detail::new("http_non_unary").with_message("HTTP supports unary capabilities only")
        );
    }
    for (slot, ty) in [
        ("input", descriptor.input()),
        ("output", descriptor.output()),
        ("error", descriptor.error()),
    ] {
        if matches!(ty.view(), DescriptorRef::TriState(_)) {
            return Err(Detail::new("http_top_level_field")
                .with_message(format!("HTTP cannot represent top-level Field in {slot}")));
        }
    }
    if [descriptor.input(), descriptor.output(), descriptor.error()]
        .into_iter()
        .any(|ty| secret_contains_presence(ty, false))
    {
        return Err(Detail::new("http_secret_presence")
            .with_message("HTTP cannot represent presence inside Secret"));
    }
    Ok(())
}

fn secret_contains_presence(descriptor: &TypeDescriptor, inside_secret: bool) -> bool {
    match descriptor.view() {
        DescriptorRef::Optional(inner) | DescriptorRef::TriState(inner) => {
            inside_secret || secret_contains_presence(inner, inside_secret)
        }
        DescriptorRef::Secret(inner) => secret_contains_presence(inner, true),
        DescriptorRef::List(inner) | DescriptorRef::Map(inner) => {
            secret_contains_presence(inner, inside_secret)
        }
        DescriptorRef::Struct(fields) => fields
            .iter()
            .any(|field| secret_contains_presence(field.descriptor(), inside_secret)),
        DescriptorRef::Enum(variants) => variants.iter().any(|variant| match variant.payload() {
            VariantPayload::Unit => false,
            VariantPayload::Value(inner) => secret_contains_presence(inner, inside_secret),
        }),
        _ => false,
    }
}
