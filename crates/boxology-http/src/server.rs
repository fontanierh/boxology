use crate::encoder::WireCallError;
use boxology_contract::{
    BoxId, CapabilityDescriptor, CapabilityName, CapabilityShape, DescriptorRef, Detail,
    ExposureLevel, TypeDescriptor, VariantPayload,
};
use boxology_runtime::TransportExposure;

trait ExposureView {
    fn descriptor(&self) -> &CapabilityDescriptor;
    fn level(&self) -> ExposureLevel;
}

impl ExposureView for TransportExposure {
    fn descriptor(&self) -> &CapabilityDescriptor {
        self.descriptor()
    }

    fn level(&self) -> ExposureLevel {
        self.level()
    }
}

fn resolve_route<'a, E: ExposureView>(
    raw_path: &str,
    query_present: bool,
    exposures: &'a [E],
) -> Result<&'a E, WireCallError> {
    let Some(rest) = raw_path.strip_prefix("/rpc/") else {
        return Err(WireCallError::UnknownBox);
    };
    let (box_segment, capability_segment) = rest.split_once('/').unwrap_or((rest, ""));
    let box_id = BoxId::new(box_segment).map_err(|_| WireCallError::UnknownBox)?;
    let box_known = exposures
        .iter()
        .any(|exposure| exposure.descriptor().id().box_id() == &box_id);
    if !box_known {
        return Err(WireCallError::UnknownBox);
    }
    if capability_segment.contains('/') {
        return Err(WireCallError::UnknownCapability);
    }
    let capability =
        CapabilityName::new(capability_segment).map_err(|_| WireCallError::UnknownCapability)?;
    let exposure = exposures
        .iter()
        .find(|exposure| {
            let id = exposure.descriptor().id();
            id.box_id() == &box_id && id.name() == &capability
        })
        .ok_or(WireCallError::UnknownCapability)?;
    if query_present {
        return Err(WireCallError::InvalidRequest);
    }
    Ok(exposure)
}

fn conform_exposure(descriptor: &CapabilityDescriptor) -> Result<(), Detail> {
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
    for ty in [descriptor.input(), descriptor.output(), descriptor.error()] {
        if secret_contains_presence(ty, false) {
            return Err(Detail::new("http_secret_presence")
                .with_message("HTTP cannot represent presence inside Secret"));
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use boxology_contract::{CapabilityId, FieldDescriptor, Idempotency, VariantDescriptor};

    #[derive(Debug, PartialEq, Eq)]
    struct Exposure {
        descriptor: CapabilityDescriptor,
        level: ExposureLevel,
    }

    impl ExposureView for Exposure {
        fn descriptor(&self) -> &CapabilityDescriptor {
            &self.descriptor
        }
        fn level(&self) -> ExposureLevel {
            self.level
        }
    }

    fn capability(box_id: &str, name: &str, input: TypeDescriptor) -> CapabilityDescriptor {
        CapabilityDescriptor::new(
            CapabilityId::new(
                BoxId::new(box_id).unwrap(),
                CapabilityName::new(name).unwrap(),
            ),
            input,
            TypeDescriptor::string(),
            TypeDescriptor::enumeration([]).unwrap(),
            CapabilityShape::Unary,
            ExposureLevel::External,
            Idempotency::None,
            None,
        )
    }

    fn exposure(box_id: &str, name: &str, level: ExposureLevel) -> Exposure {
        Exposure {
            descriptor: capability(box_id, name, TypeDescriptor::string()),
            level,
        }
    }

    fn with_slots(
        shape: CapabilityShape,
        input: TypeDescriptor,
        output: TypeDescriptor,
        error: TypeDescriptor,
    ) -> CapabilityDescriptor {
        let mut descriptor = capability("box", "call", input);
        descriptor = CapabilityDescriptor::new(
            descriptor.id().clone(),
            descriptor.input().clone(),
            output,
            error,
            shape,
            ExposureLevel::External,
            Idempotency::None,
            None,
        );
        descriptor
    }

    #[test]
    fn exact_route_returns_the_selected_exposure_and_runtime_seam_exists() {
        fn actual_runtime_exposure_uses_view<T: ExposureView>() {}
        actual_runtime_exposure_uses_view::<TransportExposure>();
        let exposures = [
            exposure("alpha", "read", ExposureLevel::Internal),
            exposure("alpha", "write", ExposureLevel::External),
            exposure("beta", "read", ExposureLevel::CodeOnly),
        ];
        let selected = resolve_route("/rpc/alpha/write", false, &exposures).unwrap();
        assert_eq!(selected.descriptor().id().to_string(), "alpha.write");
        assert_eq!(selected.level(), ExposureLevel::External);
        assert_eq!(
            resolve_route("/rpc/beta/read", false, &exposures)
                .unwrap()
                .level(),
            ExposureLevel::CodeOnly
        );
    }

    #[test]
    fn malformed_and_unknown_routes_have_canonical_distinct_errors() {
        let exposures = [exposure("known", "call", ExposureLevel::Internal)];
        for path in [
            "",
            "/",
            "/RPC/known/call",
            "/rpc",
            "/rpc/",
            "/rpc//call",
            "/rpc/Known/call",
            "/rpc/known%2fother/call",
            "/rpc/ghost/call",
            "/rpc_ignored/known/call",
        ] {
            assert_eq!(
                resolve_route(path, false, &exposures),
                Err(WireCallError::UnknownBox),
                "{path}"
            );
        }
        for path in [
            "/rpc/known",
            "/rpc/known/",
            "/rpc/known/Call",
            "/rpc/known/call%20",
            "/rpc/known/ghost",
            "/rpc/known/call/",
            "/rpc/known/call/extra",
            "/rpc/known//call",
        ] {
            assert_eq!(
                resolve_route(path, false, &exposures),
                Err(WireCallError::UnknownCapability),
                "{path}"
            );
        }
        let box_error = WireCallError::UnknownBox.encode();
        let capability_error = WireCallError::UnknownCapability.encode();
        assert_eq!(box_error.status(), 404);
        assert_eq!(capability_error.status(), 404);
        assert_ne!(box_error.body(), capability_error.body());
        assert_eq!(
            box_error.body(),
            br#"{"error":{"kind":"call","code":"unknown_box","message":"unknown box"}}"#
        );
        assert_eq!(capability_error.body(), br#"{"error":{"kind":"call","code":"unknown_capability","message":"unknown capability"}}"#);
    }

    #[test]
    fn route_precedes_query_validation() {
        let exposures = [exposure("known", "call", ExposureLevel::Internal)];
        assert_eq!(
            resolve_route("/rpc/known/call", true, &exposures),
            Err(WireCallError::InvalidRequest)
        );
        assert_eq!(
            resolve_route("/rpc/known/ghost", true, &exposures),
            Err(WireCallError::UnknownCapability)
        );
        assert_eq!(
            resolve_route("/rpc/ghost/call", true, &exposures),
            Err(WireCallError::UnknownBox)
        );
    }

    #[test]
    fn rejects_every_non_unary_shape_before_presence() {
        let tri = TypeDescriptor::tri_state(TypeDescriptor::string()).unwrap();
        for shape in [
            CapabilityShape::ServerStreaming,
            CapabilityShape::ClientStreaming,
            CapabilityShape::BidirectionalStreaming,
            CapabilityShape::EventSubscription,
        ] {
            let error = conform_exposure(&with_slots(shape, tri.clone(), tri.clone(), tri.clone()))
                .unwrap_err();
            assert_eq!(error.code(), "http_non_unary");
        }
    }

    #[test]
    fn top_level_field_is_rejected_in_each_slot_with_stable_precedence() {
        let plain = || TypeDescriptor::string();
        let field = || TypeDescriptor::tri_state(plain()).unwrap();
        for (descriptor, slot) in [
            (
                with_slots(CapabilityShape::Unary, field(), plain(), plain()),
                "input",
            ),
            (
                with_slots(CapabilityShape::Unary, plain(), field(), plain()),
                "output",
            ),
            (
                with_slots(CapabilityShape::Unary, plain(), plain(), field()),
                "error",
            ),
        ] {
            let error = conform_exposure(&descriptor).unwrap_err();
            assert_eq!(error.code(), "http_top_level_field");
            assert_eq!(
                error.message(),
                Some(format!("HTTP cannot represent top-level Field in {slot}").as_str())
            );
        }
        let all = with_slots(CapabilityShape::Unary, field(), field(), field());
        assert_eq!(
            conform_exposure(&all).unwrap_err().message(),
            Some("HTTP cannot represent top-level Field in input")
        );
    }

    #[test]
    fn secret_rejects_deep_presence_across_all_aggregate_kinds_without_leaking_names() {
        let optional = TypeDescriptor::optional(TypeDescriptor::string()).unwrap();
        let enumeration = TypeDescriptor::enumeration([VariantDescriptor::new(
            "variant-sentinel",
            VariantPayload::Value(optional),
            None,
        )])
        .unwrap();
        let nested = TypeDescriptor::structure([FieldDescriptor::new(
            "payload-sentinel",
            TypeDescriptor::list(TypeDescriptor::map(enumeration).unwrap()).unwrap(),
            None,
        )])
        .unwrap();
        let error = conform_exposure(&with_slots(
            CapabilityShape::Unary,
            TypeDescriptor::string(),
            TypeDescriptor::secret(nested).unwrap(),
            TypeDescriptor::string(),
        ))
        .unwrap_err();
        assert_eq!(error.code(), "http_secret_presence");
        assert_eq!(
            error.message(),
            Some("HTTP cannot represent presence inside Secret")
        );
        assert!(!error.to_string().contains("sentinel"));

        let tri_in_struct = TypeDescriptor::structure([FieldDescriptor::new(
            "field",
            TypeDescriptor::tri_state(TypeDescriptor::bool()).unwrap(),
            None,
        )])
        .unwrap();
        let error_slot = TypeDescriptor::secret(tri_in_struct).unwrap();
        assert_eq!(
            conform_exposure(&with_slots(
                CapabilityShape::Unary,
                TypeDescriptor::string(),
                TypeDescriptor::string(),
                error_slot
            ))
            .unwrap_err()
            .code(),
            "http_secret_presence"
        );
    }

    #[test]
    fn accepts_supported_presence_and_secret_shapes() {
        let object_field = TypeDescriptor::structure([FieldDescriptor::new(
            "field",
            TypeDescriptor::tri_state(TypeDescriptor::bool()).unwrap(),
            None,
        )])
        .unwrap();
        for input in [
            TypeDescriptor::string(),
            TypeDescriptor::optional(TypeDescriptor::string()).unwrap(),
            object_field,
            TypeDescriptor::secret(TypeDescriptor::string()).unwrap(),
            TypeDescriptor::optional(TypeDescriptor::secret(TypeDescriptor::string()).unwrap())
                .unwrap(),
        ] {
            conform_exposure(&with_slots(
                CapabilityShape::Unary,
                input,
                TypeDescriptor::string(),
                TypeDescriptor::string(),
            ))
            .unwrap();
        }
    }
}
