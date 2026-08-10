use super::{
    Diagnostic, DiagnosticCode, Diagnostics, GenerationRequest, LineColumn, POINT, RelativePath,
    Span,
};
use boxology_contract::{BoxId, CapabilityId, CapabilityName, ExposureLevel, Idempotency};
use std::collections::BTreeSet;
use syn::ext::IdentExt;
use syn::visit::Visit;

const RULE: &str = "every declared .rs input must parse as a complete Rust file";
const RULE_SOURCE: &str = "specs/s2-contract-generator.md D2";
const PATH_RULE: &str = "#[path] module overrides are not supported in v0";
const MISSING_RULE: &str =
    "outline module lookup must find x.rs or x/mod.rs among declared Rust inputs";
const AMBIGUOUS_RULE: &str =
    "outline module lookup must not find both x.rs and x/mod.rs among declared Rust inputs";
const UNREACHABLE_RULE: &str =
    "Boxology-annotated items must be reachable from the declared crate root";
const CONDITIONAL_RULE: &str = "cfg and cfg_attr are forbidden on exported items, their fields or variants, surrounding impls, and ancestor module declarations";
const COLLISION_RULE: &str = "contract type names must be unique in the flat lifted namespace";
const COLLISION_RULE_SOURCE: &str = "specs/s2-contract-generator.md D2-D4";
const ATTRIBUTE_RULE: &str = "contract declarations, their fields, and variants may use only doc, direct boxology attributes, deprecated, and derive";
const DERIVE_RULE: &str =
    "contract declarations, their fields, and variants may derive only Debug, Clone, and PartialEq";
const CONTRACT_ROLE_RULE: &str =
    "contract declarations use #[boxology::contract]; only enums may use the single error marker";
const CONTRACT_ROLE_RULE_SOURCE: &str = "specs/s2-contract-generator.md D1-D4";
const DEPRECATION_RULE: &str = "deprecation may appear at most once per exported type, field, or variant as #[deprecated] or #[deprecated(note = \"...\")]";
const DEPRECATION_RULE_SOURCE: &str = "specs/s2-contract-generator.md D2,D5a";
const DOCUMENTATION_RULE: &str = "documentation attributes must use #[doc = \"...\"] with a direct string literal and no expression attributes";
const DOCUMENTATION_RULE_SOURCE: &str = "specs/s2-contract-generator.md D2";
const FIELD_IDENTITY_RULE: &str =
    "named field identities must be unique within each immediate contract field container";
const VARIANT_IDENTITY_RULE: &str = "variant identities must be unique within each contract enum";
const MEMBER_IDENTITY_RULE_SOURCE: &str = "specs/s2-contract-generator.md D4";
const CONTRACT_PLACEMENT_RULE: &str = "direct boxology::contract annotations are allowed only on reachable module-scope structs and enums";
const CONTRACT_PLACEMENT_RULE_SOURCE: &str = "specs/s2-contract-generator.md D1-D2";
const CAPABILITY_PLACEMENT_RULE: &str =
    "direct boxology::capability annotations are allowed only on functions in inherent impls";
const CAPABILITY_PLACEMENT_RULE_SOURCE: &str = "specs/s2-contract-generator.md D1,D8";
const CAPABILITY_CALL_SHAPE_RULE: &str = "v0 capabilities must be async methods with a shared &self receiver, exactly two typed parameters after the receiver, no variadic parameter, and an explicit return type";
const CAPABILITY_CALL_SHAPE_RULE_SOURCE: &str = "specs/s2-contract-generator.md D1,D8";
const CAPABILITY_METADATA_RULE: &str = "capability metadata supports name, exposure, and idempotency with the v0 values declared by S2";
const CAPABILITY_METADATA_RULE_SOURCE: &str = "specs/s2-contract-generator.md D1,D3";
const CAPABILITY_NAME_RULE: &str =
    "capability local names must match [a-z][a-z0-9_]* after applying an optional name override";
const CAPABILITY_IDENTITY_RULE: &str = "effective capability names must be unique within a box; the first declaration in deterministic declaration order owns the identity";
const CAPABILITY_IDENTITY_RULE_SOURCE: &str = "specs/s2-contract-generator.md D4";
const CONTROLLED_SITE_RULE: &str =
    "exact boxology::contract! invocations must appear once at reachable module scope";
const CONTROLLED_PARSE_RULE: &str = "contract tokens must satisfy the controlled v0 grammar";
const CONTROLLED_PARSE_RULE_SOURCE: &str = "specs/s2-contract-generator.md D3";
const EMITTABLE_RULE: &str = "the `Blob` capability boundary or value-payload leaf is parsed and modelled but its v0 end-to-end runtime generation is not yet implemented (deferred); scalar leaves and `String` are emittable.";
const EMITTABLE_RULE_SOURCE: &str = "specs/s2-contract-generator.md D3,D5";
const PAYLOAD_EMITTABLE_RULE: &str = "named-field payloads require contract-emitter support";
const PAYLOAD_EMITTABLE_RULE_SOURCE: &str = "specs/s2-contract-generator.md D3";

/// Every successfully parsed Rust input, sorted by logical-path bytes.
pub struct ParsedRustInputs {
    box_id: BoxId,
    inputs: Vec<ParsedRustInput>,
    crate_root: usize,
}

/// One logical Rust input and its complete parsed syntax tree.
pub struct ParsedRustInput {
    path: RelativePath,
    syntax: syn::File,
}

/// One validated controlled contract discovered from the cold logical source tree.
pub struct ControlledContract {
    source: RelativePath,
    span: Span,
    model: boxology_contract_syntax::Contract,
    capability_id: CapabilityId,
    canonical_semantic_bytes: Vec<u8>,
    semantic_digest: [u8; 32],
}
/// A borrowed capability method found by structural declaration discovery.
pub struct CapabilityDeclaration<'ast> {
    source: &'ast RelativePath,
    module_path: Vec<String>,
    identifier_span: Span,
    implementation: &'ast syn::ItemImpl,
    method: &'ast syn::ImplItemFn,
    marker_metadata: Option<CapabilityMarkerMetadata>,
    identity: Option<CapabilityId>,
}

/// Validated owned metadata projected from one capability marker.
pub struct CapabilityMarkerMetadata {
    name_override: Option<String>,
    name_override_span: Option<Span>,
    max_exposure: ExposureLevel,
    idempotency: Idempotency,
}

/// A reachable module-scope contract struct or enum found by provisional declaration discovery.
pub struct ContractDeclaration<'a> {
    source: &'a RelativePath,
    identifier_span: Span,
    module_path: Vec<String>,
    lifted_name: String,
    syntax: ContractDeclarationSyntax<'a>,
    role: ContractDeclarationRole,
    projection: Option<(ContractDeclarationShape<'a>, ContractSiteMetadata)>,
}

/// Metadata attached to one contract site.
pub struct ContractSiteMetadata {
    deprecation: Option<ContractDeprecation>,
    docs: Vec<String>,
}

macro_rules! model_getters {
    ($this:ident; $(#[$meta:meta] $name:ident: $return:ty = $body:expr;)*) => {$(
        #[$meta] pub fn $name(&$this) -> $return { $body }
    )*};
}

impl ControlledContract {
    model_getters! { self;
        #[doc = "Returns the contract invocation's logical source path."]
        source: &RelativePath = &self.source;
        #[doc = "Returns the direct contract-leaf source span."]
        span: Span = self.span;
        #[doc = "Returns the shared-parser semantic model."]
        model: &boxology_contract_syntax::Contract = &self.model;
        #[doc = "Returns the box-qualified capability identity, excluded from the semantic digest."]
        capability_id: &CapabilityId = &self.capability_id;
        #[doc = "Returns the canonical generation-consistency bytes computed after parsing."]
        canonical_semantic_bytes: &[u8] = &self.canonical_semantic_bytes;
        #[doc = "Returns the SHA-256 generation-consistency digest computed after parsing."]
        semantic_digest: &[u8; 32] = &self.semantic_digest;
    }

    /// Fails closed when parsed semantics are not yet supported by the v0 emitter.
    ///
    /// Scalar leaves, `String`, and the accepted narrow structured subset are emittable, including
    /// scalar one-value error payloads; the plain parse path still returns `Blob` and named-payload
    /// models so later tasks can consume them while this guard prevents unsupported artifacts.
    /// Contracts holding any number of capabilities are emittable; the guard checks every
    /// capability's boundary leaves and every value-payload leaf.
    ///
    /// # Errors
    /// Returns at most one diagnostic per unsupported family at the contract-invocation span.
    pub fn require_v0_emittable(&self) -> Result<(), Diagnostics> {
        let has_blob_boundary = self.model.capabilities.iter().any(|capability| {
            capability.input_type.contains_blob() || capability.output_type.contains_blob()
        }) || self.model.data.iter().any(|declaration| match &declaration
            .shape
        {
            boxology_contract_syntax::DataShape::Struct(fields) => {
                fields.iter().any(|field| field.ty.contains_blob())
            }
            boxology_contract_syntax::DataShape::Enum(_) => false,
        }) || self.model.error.variants.iter().any(|variant| {
            matches!(
                &variant.payload,
                boxology_contract_syntax::VariantPayload::Value(value) if value.ty.is_blob()
            )
        });
        let has_named_payload = self.model.error.variants.iter().any(|variant| {
            matches!(
                &variant.payload,
                boxology_contract_syntax::VariantPayload::Named(_)
            )
        });
        let mut diagnostics = Vec::new();
        if has_blob_boundary {
            diagnostics.push(Diagnostic {
                path: self.source.clone(),
                span: self.span,
                code: DiagnosticCode::Bxg0040,
                offending: "Blob capability boundary or value-payload leaf not yet emittable in v0"
                    .into(),
                rule: EMITTABLE_RULE,
                rule_source: EMITTABLE_RULE_SOURCE,
            });
        }
        if has_named_payload {
            diagnostics.push(Diagnostic {
                path: self.source.clone(),
                span: self.span,
                code: DiagnosticCode::Bxg0048,
                offending: "named-field error variants are not yet emittable".into(),
                rule: PAYLOAD_EMITTABLE_RULE,
                rule_source: PAYLOAD_EMITTABLE_RULE_SOURCE,
            });
        }
        if diagnostics.is_empty() {
            Ok(())
        } else {
            diagnostics.sort();
            diagnostics.dedup();
            Err(Diagnostics(diagnostics))
        }
    }
}
impl<'ast> CapabilityDeclaration<'ast> {
    model_getters! { self;
        #[doc = "Returns the declaration's exact logical source path."]
        source: &RelativePath = self.source;
        #[doc = "Returns the canonical unraw module components, empty at the crate root."]
        module_path: &[String] = &self.module_path;
        #[doc = "Returns the method identifier's one-based source span."]
        identifier_span: Span = self.identifier_span;
        #[doc = "Returns the borrowed owning inherent impl."]
        implementation: &'ast syn::ItemImpl = self.implementation;
        #[doc = "Returns the borrowed annotated method."]
        method: &'ast syn::ImplItemFn = self.method;
    }

    /// Returns metadata after capability-marker validation.
    pub fn marker_metadata(&self) -> &CapabilityMarkerMetadata {
        self.marker_metadata
            .as_ref()
            .expect("validated capability marker metadata")
    }

    /// Returns the identity after effective-name validation.
    pub fn id(&self) -> &CapabilityId {
        self.identity
            .as_ref()
            .expect("validated capability identity")
    }
}

impl CapabilityMarkerMetadata {
    model_getters! { self;
        #[doc = "Returns the exact optional name override without validating or defaulting it."]
        name_override: Option<&str> = self.name_override.as_deref();
        #[doc = "Returns the greatest permitted exposure."]
        max_exposure: ExposureLevel = self.max_exposure;
        #[doc = "Returns the declared idempotency property."]
        idempotency: Idempotency = self.idempotency;
    }
}

impl std::fmt::Debug for CapabilityMarkerMetadata {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CapabilityMarkerMetadata")
            .field("name_override", &self.name_override)
            .field("max_exposure", &self.max_exposure)
            .field("idempotency", &self.idempotency)
            .finish()
    }
}

impl PartialEq for CapabilityMarkerMetadata {
    fn eq(&self, other: &Self) -> bool {
        self.name_override == other.name_override
            && self.max_exposure == other.max_exposure
            && self.idempotency == other.idempotency
    }
}

impl Eq for CapabilityMarkerMetadata {}

impl ContractSiteMetadata {
    model_getters! { self;
        #[doc = "Returns the site's optional deprecation metadata."]
        deprecation: Option<&ContractDeprecation> = self.deprecation.as_ref();
    }

    /// Returns the site's decoded documentation attributes in source order.
    pub fn docs(&self) -> &[String] {
        &self.docs
    }
}

/// Validated deprecation metadata attached to one contract site.
pub struct ContractDeprecation {
    note: Option<String>,
}

impl ContractDeprecation {
    /// Returns the decoded optional deprecation note.
    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }
}

/// One source-owned field or variant identity.
pub struct ContractMemberIdentity<'ast> {
    name: String,
    ident: &'ast syn::Ident,
}

impl<'ast> ContractMemberIdentity<'ast> {
    model_getters! { self;
        #[doc = "Returns the identifier in its unraw spelling."]
        name: &str = &self.name;
        #[doc = "Returns the original parsed identifier."]
        ident: &'ast syn::Ident = self.ident;
    }
}

/// One borrowed contract field.
pub struct ContractField<'ast> {
    ordinal: usize,
    identity: Option<ContractMemberIdentity<'ast>>,
    syntax: &'ast syn::Field,
    metadata: ContractSiteMetadata,
}

impl<'ast> ContractField<'ast> {
    model_getters! { self;
        #[doc = "Returns the zero-based ordinal within the immediate field container."]
        ordinal: usize = self.ordinal;
        #[doc = "Returns the named-field identity, or `None` for tuple fields."]
        identity: Option<&ContractMemberIdentity<'ast>> = self.identity.as_ref();
        #[doc = "Returns the original parsed field."]
        syntax: &'ast syn::Field = self.syntax;
    }
    /// Returns the original, unvalidated field type syntax.
    pub fn ty(&self) -> &'ast syn::Type {
        &self.syntax.ty
    }
    model_getters! { self;
        #[doc = "Returns field metadata."]
        metadata: &ContractSiteMetadata = &self.metadata;
    }
}

/// The borrowed field shape of a contract struct or enum variant.
pub enum ContractFields<'ast> {
    /// Named fields in source order.
    Named(Vec<ContractField<'ast>>),
    /// Tuple fields in source order.
    Unnamed(Vec<ContractField<'ast>>),
    /// No fields.
    Unit,
}

/// One borrowed enum variant.
pub struct ContractVariant<'ast> {
    ordinal: usize,
    identity: ContractMemberIdentity<'ast>,
    syntax: &'ast syn::Variant,
    fields: ContractFields<'ast>,
    metadata: ContractSiteMetadata,
}

impl<'ast> ContractVariant<'ast> {
    model_getters! { self;
        #[doc = "Returns the zero-based ordinal within the enum."]
        ordinal: usize = self.ordinal;
        #[doc = "Returns the variant identity."]
        identity: &ContractMemberIdentity<'ast> = &self.identity;
        #[doc = "Returns the original parsed variant."]
        syntax: &'ast syn::Variant = self.syntax;
        #[doc = "Returns the variant field shape."]
        fields: &ContractFields<'ast> = &self.fields;
        #[doc = "Returns variant metadata."]
        metadata: &ContractSiteMetadata = &self.metadata;
    }
}

/// The complete borrowed structural shape of a contract declaration.
pub enum ContractDeclarationShape<'ast> {
    /// A struct and its field shape.
    Struct(ContractFields<'ast>),
    /// An enum and its variants in source order.
    Enum(Vec<ContractVariant<'ast>>),
}

/// The semantic role of a discovered contract declaration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContractDeclarationRole {
    /// A value contract declared by a struct or ordinary enum.
    Value,
    /// A structured-error contract declared by an enum.
    Error,
}

/// The parsed syntax belonging to a discovered contract declaration.
#[derive(Clone, Copy)]
pub enum ContractDeclarationSyntax<'a> {
    /// A contract struct declaration.
    Struct(&'a syn::ItemStruct),
    /// A contract enum declaration, including a provisionally recognized error enum.
    Enum(&'a syn::ItemEnum),
}

impl<'ast> ContractDeclaration<'ast> {
    /// Returns the declaration's exact logical source path.
    pub fn source(&self) -> &RelativePath {
        self.source
    }

    /// Returns the declaration identifier's one-based source span.
    pub fn identifier_span(&self) -> Span {
        self.identifier_span
    }

    /// Returns the canonical unraw module components, empty at the crate root.
    pub fn module_path(&self) -> &[String] {
        &self.module_path
    }

    /// Returns the owned declaration name in its unraw spelling.
    pub fn lifted_name(&self) -> &str {
        &self.lifted_name
    }

    /// Returns the declaration's parsed struct-or-enum syntax.
    pub fn syntax(&self) -> ContractDeclarationSyntax<'_> {
        self.syntax
    }

    /// Returns the declaration's validated semantic role.
    pub fn role(&self) -> ContractDeclarationRole {
        self.role
    }

    /// Returns the declaration's borrowed structural shape.
    pub fn shape(&self) -> &ContractDeclarationShape<'ast> {
        &self.projection.as_ref().expect("validated declaration").0
    }

    /// Returns declaration metadata.
    pub fn metadata(&self) -> &ContractSiteMetadata {
        &self.projection.as_ref().expect("validated declaration").1
    }
}

impl ParsedRustInputs {
    /// Parses every exact `.rs` request input and aggregates all syntax failures.
    pub fn parse(request: &GenerationRequest) -> Result<Self, Diagnostics> {
        let mut inputs = request
            .inputs()
            .iter()
            .filter(|input| input.path().as_str().ends_with(".rs"))
            .collect::<Vec<_>>();
        inputs.sort_by(|left, right| {
            left.path()
                .as_str()
                .as_bytes()
                .cmp(right.path().as_str().as_bytes())
        });
        let crate_root = inputs
            .iter()
            .position(|input| input.path() == request.crate_root())
            .expect("GenerationRequest guarantees the crate root is a declared Rust input");

        let mut parsed = Vec::with_capacity(inputs.len());
        let mut diagnostics = Vec::new();
        for input in inputs {
            let source = std::str::from_utf8(input.bytes())
                .expect("GenerationRequest guarantees Rust inputs are valid UTF-8");
            match syn::parse_file(source) {
                Ok(syntax) => parsed.push(ParsedRustInput {
                    path: input.path().clone(),
                    syntax,
                }),
                Err(error) => append_errors(input.path(), error, &mut diagnostics),
            }
        }
        if diagnostics.is_empty() {
            Ok(Self {
                box_id: request.box_id().clone(),
                inputs: parsed,
                crate_root,
            })
        } else {
            diagnostics.sort();
            Err(Diagnostics(diagnostics))
        }
    }

    /// Returns parsed inputs in logical-path byte order.
    pub fn as_slice(&self) -> &[ParsedRustInput] {
        &self.inputs
    }

    /// Discovers and parses the single reachable direct module-scope `boxology::contract!`.
    pub fn controlled_contract(&self) -> Result<ControlledContract, Diagnostics> {
        let reachable = self.resolve_reachable_inputs()?;
        let root = &self.inputs[self.crate_root];
        let mut sites = Vec::new();
        let mut diagnostics = Vec::new();
        for input in &reachable {
            collect_controlled_sites(&input.path, &input.syntax.items, &mut sites);
        }
        sites.sort_by(|left, right| {
            left.path
                .as_str()
                .as_bytes()
                .cmp(right.path.as_str().as_bytes())
                .then_with(|| {
                    source_span(left.item.mac.path.segments[1].ident.span())
                        .cmp(&source_span(right.item.mac.path.segments[1].ident.span()))
                })
        });
        let allowed = sites
            .iter()
            .map(|site| std::ptr::from_ref(&site.item.mac))
            .collect::<BTreeSet<_>>();
        for input in reachable {
            ControlledPlacementVisitor {
                path: &input.path,
                allowed: &allowed,
                diagnostics: &mut diagnostics,
            }
            .visit_file(&input.syntax);
        }
        diagnostics.sort();
        diagnostics.dedup();
        if !diagnostics.is_empty() {
            return Err(Diagnostics(diagnostics));
        }
        if sites.is_empty() {
            diagnostics.push(Diagnostic {
                path: root.path.clone(),
                span: Span {
                    start: POINT,
                    end: POINT,
                },
                code: DiagnosticCode::Bxg0037,
                offending: "missing controlled contract invocation".into(),
                rule: CONTROLLED_SITE_RULE,
                rule_source: RULE_SOURCE,
            });
        } else {
            for site in sites.iter().skip(1) {
                diagnostics.push(module_diagnostic(
                    site.path,
                    site.item.mac.path.segments[1].ident.span(),
                    DiagnosticCode::Bxg0037,
                    "additional controlled contract invocation",
                    CONTROLLED_SITE_RULE,
                ));
            }
        }
        diagnostics.sort();
        if !diagnostics.is_empty() {
            return Err(Diagnostics(diagnostics));
        }
        let site = &sites[0];
        let contract =
            boxology_contract_syntax::parse(site.item.mac.tokens.clone()).map_err(|error| {
                let mut errors = error
                    .into_iter()
                    .map(|component| Diagnostic {
                        path: site.path.clone(),
                        span: source_span(component.span()),
                        code: DiagnosticCode::Bxg0038,
                        offending: "invalid controlled contract syntax".into(),
                        rule: CONTROLLED_PARSE_RULE,
                        rule_source: CONTROLLED_PARSE_RULE_SOURCE,
                    })
                    .collect::<Vec<_>>();
                errors.sort();
                errors.dedup();
                Diagnostics(errors)
            })?;
        let name = CapabilityName::new(contract.capabilities[0].name.clone())
            .expect("the shared parser validates capability identity grammar");
        let (canonical_semantic_bytes, semantic_digest) =
            boxology_contract_syntax::semantic_artifacts(&contract);
        Ok(ControlledContract {
            source: site.path.clone(),
            span: source_span(site.item.mac.path.segments[1].ident.span()),
            capability_id: CapabilityId::new(self.box_id.clone(), name),
            model: contract,
            canonical_semantic_bytes,
            semantic_digest,
        })
    }

    /// Provisionally discovers reachable contract structs and enums and rejects lifted-name collisions.
    ///
    /// This phase intentionally ignores deferred placements and is not complete authoring-grammar
    /// validation.
    pub fn discover_contract_declarations(
        &self,
    ) -> Result<Vec<ContractDeclaration<'_>>, Diagnostics> {
        let reachable = self.resolve_reachable_inputs()?;
        let root = &self.inputs[self.crate_root];
        let module_dir = root
            .path
            .as_str()
            .rsplit_once('/')
            .map_or("", |pair| pair.0);
        let mut visited = vec![false; self.inputs.len()];
        visited[self.crate_root] = true;
        let mut declarations = Vec::new();
        let mut capabilities = Vec::new();
        self.collect_contract_declarations(
            self.crate_root,
            &root.syntax.items,
            module_dir,
            &mut Vec::new(),
            &mut visited,
            &mut declarations,
            &mut capabilities,
        );
        declarations.sort_by(|left, right| {
            left.module_path
                .cmp(&right.module_path)
                .then_with(|| {
                    left.source
                        .as_str()
                        .as_bytes()
                        .cmp(right.source.as_str().as_bytes())
                })
                .then(left.identifier_span.cmp(&right.identifier_span))
        });

        let mut names = BTreeSet::new();
        let mut diagnostics = Vec::new();
        for declaration in &declarations {
            if !names.insert(declaration.lifted_name.clone()) {
                diagnostics.push(Diagnostic {
                    path: declaration.source.clone(),
                    span: declaration.identifier_span,
                    code: DiagnosticCode::Bxg0021,
                    offending: "colliding lifted contract type name".into(),
                    rule: COLLISION_RULE,
                    rule_source: COLLISION_RULE_SOURCE,
                });
            }
        }
        diagnostics.sort();
        diagnostics.dedup();
        if !diagnostics.is_empty() {
            return Err(Diagnostics(diagnostics));
        }
        for declaration in &declarations {
            validate_contract_attributes(declaration, &mut diagnostics);
        }
        diagnostics.sort();
        diagnostics.dedup();
        if !diagnostics.is_empty() {
            return Err(Diagnostics(diagnostics));
        }
        for declaration in &mut declarations {
            validate_contract_role(declaration, &mut diagnostics);
        }
        diagnostics.sort();
        diagnostics.dedup();
        if !diagnostics.is_empty() {
            return Err(Diagnostics(diagnostics));
        }
        for declaration in &declarations {
            validate_contract_deprecations(declaration, &mut diagnostics);
        }
        diagnostics.sort();
        diagnostics.dedup();
        if !diagnostics.is_empty() {
            return Err(Diagnostics(diagnostics));
        }
        for declaration in &declarations {
            validate_contract_documentation(declaration, &mut diagnostics);
        }
        diagnostics.sort();
        diagnostics.dedup();
        if !diagnostics.is_empty() {
            return Err(Diagnostics(diagnostics));
        }
        for declaration in &declarations {
            validate_member_identities(declaration, &mut diagnostics);
        }
        diagnostics.sort();
        diagnostics.dedup();
        if !diagnostics.is_empty() {
            return Err(Diagnostics(diagnostics));
        }
        validate_contract_placement(&reachable, &declarations, &mut diagnostics);
        diagnostics.sort();
        diagnostics.dedup();
        if !diagnostics.is_empty() {
            return Err(Diagnostics(diagnostics));
        }
        for declaration in &mut declarations {
            declaration.projection = Some(project_declaration(declaration.syntax));
        }
        Ok(declarations)
    }

    /// Discovers structurally placed capability methods in deterministic reachable modules.
    pub fn discover_capability_declarations(
        &self,
    ) -> Result<Vec<CapabilityDeclaration<'_>>, Diagnostics> {
        let _ = self.discover_contract_declarations()?;
        let reachable = self.resolve_reachable_inputs()?;
        let root = &self.inputs[self.crate_root];
        let module_dir = root
            .path
            .as_str()
            .rsplit_once('/')
            .map_or("", |pair| pair.0);
        let mut visited = vec![false; self.inputs.len()];
        visited[self.crate_root] = true;
        let mut contracts = Vec::new();
        let mut declarations = Vec::new();
        self.collect_contract_declarations(
            self.crate_root,
            &root.syntax.items,
            module_dir,
            &mut Vec::new(),
            &mut visited,
            &mut contracts,
            &mut declarations,
        );
        declarations.sort_by(|left, right| {
            left.module_path
                .cmp(&right.module_path)
                .then_with(|| {
                    left.source
                        .as_str()
                        .as_bytes()
                        .cmp(right.source.as_str().as_bytes())
                })
                .then(left.identifier_span.cmp(&right.identifier_span))
        });
        let mut diagnostics = Vec::new();
        validate_capability_placement(&reachable, &declarations, &mut diagnostics);
        diagnostics.sort();
        diagnostics.dedup();
        if diagnostics.is_empty() {
            Ok(declarations)
        } else {
            Err(Diagnostics(diagnostics))
        }
    }

    /// Validates the structural call frame of every discovered capability method.
    pub fn validate_capability_call_shapes(
        &self,
    ) -> Result<Vec<CapabilityDeclaration<'_>>, Diagnostics> {
        let declarations = self.discover_capability_declarations()?;
        let mut diagnostics = declarations
            .iter()
            .filter(|declaration| !valid_capability_call_shape(declaration.method))
            .map(|declaration| Diagnostic {
                path: declaration.source.clone(),
                span: declaration.identifier_span,
                code: DiagnosticCode::Bxg0031,
                offending: "invalid structural capability signature".into(),
                rule: CAPABILITY_CALL_SHAPE_RULE,
                rule_source: CAPABILITY_CALL_SHAPE_RULE_SOURCE,
            })
            .collect::<Vec<_>>();
        diagnostics.sort();
        diagnostics.dedup();
        if diagnostics.is_empty() {
            Ok(declarations)
        } else {
            Err(Diagnostics(diagnostics))
        }
    }

    /// Validates and projects the supported v0 metadata of every capability declaration.
    pub fn validate_capability_marker_metadata(
        &self,
    ) -> Result<Vec<CapabilityDeclaration<'_>>, Diagnostics> {
        let mut declarations = self.validate_capability_call_shapes()?;
        let mut diagnostics = Vec::new();
        let metadata = declarations
            .iter()
            .map(|declaration| parse_capability_metadata(declaration, &mut diagnostics))
            .collect::<Vec<_>>();
        diagnostics.sort();
        diagnostics.dedup();
        if !diagnostics.is_empty() {
            return Err(Diagnostics(diagnostics));
        }
        for (declaration, metadata) in declarations.iter_mut().zip(metadata) {
            declaration.marker_metadata = Some(metadata);
        }
        Ok(declarations)
    }

    /// Validates and projects box-qualified capability identities.
    pub fn validate_capability_identities(
        &self,
    ) -> Result<Vec<CapabilityDeclaration<'_>>, Diagnostics> {
        let mut declarations = self.validate_capability_marker_metadata()?;
        let mut diagnostics = Vec::new();
        let names = declarations
            .iter()
            .map(|declaration| {
                let metadata = declaration.marker_metadata();
                let (name, span) = metadata.name_override.as_ref().map_or_else(
                    || {
                        (
                            declaration.method.sig.ident.unraw().to_string(),
                            declaration.identifier_span,
                        )
                    },
                    |name| (name.clone(), metadata.name_override_span.unwrap()),
                );
                CapabilityName::new(name).map_err(|_| {
                    diagnostics.push(capability_identity_error(
                        declaration,
                        span,
                        DiagnosticCode::Bxg0034,
                    ));
                })
            })
            .collect::<Vec<_>>();
        diagnostics.sort();
        diagnostics.dedup();
        if !diagnostics.is_empty() {
            return Err(Diagnostics(diagnostics));
        }
        let names = names.into_iter().map(Result::unwrap).collect::<Vec<_>>();
        let mut seen = BTreeSet::new();
        for (declaration, name) in declarations.iter().zip(&names) {
            if !seen.insert(name.clone()) {
                let span = declaration
                    .marker_metadata()
                    .name_override_span
                    .unwrap_or(declaration.identifier_span);
                diagnostics.push(capability_identity_error(
                    declaration,
                    span,
                    DiagnosticCode::Bxg0035,
                ));
            }
        }
        diagnostics.sort();
        diagnostics.dedup();
        if !diagnostics.is_empty() {
            return Err(Diagnostics(diagnostics));
        }
        for (declaration, name) in declarations.iter_mut().zip(names) {
            declaration.identity = Some(CapabilityId::new(self.box_id.clone(), name));
        }
        Ok(declarations)
    }

    /// Validates default module lookup and returns unique reachable files in logical-path byte order.
    pub fn resolve_reachable_inputs(&self) -> Result<Vec<&ParsedRustInput>, Diagnostics> {
        let root = &self.inputs[self.crate_root];
        let module_dir = root
            .path
            .as_str()
            .rsplit_once('/')
            .map_or("", |pair| pair.0);
        let mut reachable = vec![false; self.inputs.len()];
        reachable[self.crate_root] = true;
        let mut diagnostics = Vec::new();
        self.visit_items(
            self.crate_root,
            &root.syntax.items,
            module_dir,
            &mut reachable,
            &mut diagnostics,
        );
        if !diagnostics.is_empty() {
            diagnostics.sort();
            return Err(Diagnostics(diagnostics));
        }

        for (source, input) in self.inputs.iter().enumerate() {
            if !reachable[source] {
                self.inspect_unreachable(source, &input.syntax.items, &mut diagnostics);
            }
        }
        self.validate_items(
            self.crate_root,
            &root.syntax.items,
            module_dir,
            &mut Vec::new(),
            &mut diagnostics,
        );
        diagnostics.sort();
        diagnostics.dedup();
        if !diagnostics.is_empty() {
            return Err(Diagnostics(diagnostics));
        }
        Ok(self
            .inputs
            .iter()
            .zip(reachable)
            .filter_map(|(input, reachable)| reachable.then_some(input))
            .collect())
    }

    fn visit_items(
        &self,
        source: usize,
        items: &[syn::Item],
        module_dir: &str,
        reachable: &mut [bool],
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        for module in items.iter().filter_map(|item| match item {
            syn::Item::Mod(module) => Some(module),
            _ => None,
        }) {
            let mut has_path = false;
            for attribute in &module.attrs {
                let Some(identifier) = attribute.path().get_ident() else {
                    continue;
                };
                if identifier.unraw() == "path" {
                    has_path = true;
                    diagnostics.push(module_diagnostic(
                        &self.inputs[source].path,
                        identifier.span(),
                        DiagnosticCode::Bxg0016,
                        "module path override",
                        PATH_RULE,
                    ));
                }
            }
            if has_path {
                continue;
            }

            let name = module.ident.unraw().to_string();
            let child_dir = if module_dir.is_empty() {
                name
            } else {
                format!("{module_dir}/{name}")
            };
            if let Some((_, items)) = &module.content {
                self.visit_items(source, items, &child_dir, reachable, diagnostics);
                continue;
            }

            let direct = self.find(&format!("{child_dir}.rs"));
            let nested = self.find(&format!("{child_dir}/mod.rs"));
            let target = match (direct, nested) {
                (None, None) => {
                    diagnostics.push(module_diagnostic(
                        &self.inputs[source].path,
                        module.ident.span(),
                        DiagnosticCode::Bxg0017,
                        "missing outline module input",
                        MISSING_RULE,
                    ));
                    continue;
                }
                (Some(_), Some(_)) => {
                    diagnostics.push(module_diagnostic(
                        &self.inputs[source].path,
                        module.ident.span(),
                        DiagnosticCode::Bxg0018,
                        "ambiguous outline module inputs",
                        AMBIGUOUS_RULE,
                    ));
                    continue;
                }
                (Some(target), None) | (None, Some(target)) => target,
            };
            if !reachable[target] {
                reachable[target] = true;
                self.visit_items(
                    target,
                    &self.inputs[target].syntax.items,
                    &child_dir,
                    reachable,
                    diagnostics,
                );
            }
        }
    }

    fn find(&self, path: &str) -> Option<usize> {
        self.inputs
            .binary_search_by(|input| input.path.as_str().as_bytes().cmp(path.as_bytes()))
            .ok()
    }

    #[allow(clippy::too_many_arguments)]
    fn collect_contract_declarations<'a>(
        &'a self,
        source: usize,
        items: &'a [syn::Item],
        module_dir: &str,
        module_path: &mut Vec<String>,
        visited: &mut [bool],
        declarations: &mut Vec<ContractDeclaration<'a>>,
        capabilities: &mut Vec<CapabilityDeclaration<'a>>,
    ) {
        for item in items {
            if !matches!(item, syn::Item::Mod(_)) {
                CapabilityCollector {
                    source: &self.inputs[source].path,
                    module_path: module_path.clone(),
                    implementation: None,
                    declarations: capabilities,
                }
                .visit_item(item);
            }
            let declaration = match item {
                syn::Item::Struct(item) if has_boxology(&item.attrs, "contract") => {
                    Some((&item.ident, ContractDeclarationSyntax::Struct(item)))
                }
                syn::Item::Enum(item) if has_boxology(&item.attrs, "contract") => {
                    Some((&item.ident, ContractDeclarationSyntax::Enum(item)))
                }
                _ => None,
            };
            if let Some((identifier, syntax)) = declaration {
                declarations.push(ContractDeclaration {
                    source: &self.inputs[source].path,
                    identifier_span: source_span(identifier.span()),
                    module_path: module_path.clone(),
                    lifted_name: identifier.unraw().to_string(),
                    syntax,
                    role: ContractDeclarationRole::Value,
                    projection: None,
                });
            }

            let syn::Item::Mod(module) = item else {
                continue;
            };
            let name = module.ident.unraw().to_string();
            let child_dir = if module_dir.is_empty() {
                name.clone()
            } else {
                format!("{module_dir}/{name}")
            };
            module_path.push(name);
            if let Some((_, items)) = &module.content {
                self.collect_contract_declarations(
                    source,
                    items,
                    &child_dir,
                    module_path,
                    visited,
                    declarations,
                    capabilities,
                );
            } else {
                let target = self
                    .find(&format!("{child_dir}.rs"))
                    .or_else(|| self.find(&format!("{child_dir}/mod.rs")))
                    .expect("structural validation guarantees one outline target");
                if !visited[target] {
                    visited[target] = true;
                    self.collect_contract_declarations(
                        target,
                        &self.inputs[target].syntax.items,
                        &child_dir,
                        module_path,
                        visited,
                        declarations,
                        capabilities,
                    );
                }
            }
            module_path.pop();
        }
    }

    fn inspect_unreachable(
        &self,
        source: usize,
        items: &[syn::Item],
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        for item in items {
            UnreachableVisitor {
                path: &self.inputs[source].path,
                diagnostics,
            }
            .visit_item(item);
        }
    }

    fn validate_items<'a>(
        &'a self,
        source: usize,
        items: &'a [syn::Item],
        module_dir: &str,
        ancestors: &mut Vec<(usize, &'a syn::ItemMod)>,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        for item in items {
            if let syn::Item::Macro(item) = item
                && exact_contract_macro(&item.mac)
            {
                self.validate_context(source, &item.attrs, ancestors, diagnostics);
            }
            let attributes = item_attrs(item);
            if is_export(attributes) {
                self.validate_context(source, attributes, ancestors, diagnostics);
                if has_boxology(attributes, "contract") {
                    match item {
                        syn::Item::Struct(item) => {
                            for field in &item.fields {
                                self.add_conditionals(source, &field.attrs, diagnostics);
                            }
                        }
                        syn::Item::Enum(item) => {
                            for variant in &item.variants {
                                self.add_conditionals(source, &variant.attrs, diagnostics);
                                for field in &variant.fields {
                                    self.add_conditionals(source, &field.attrs, diagnostics);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            if !matches!(item, syn::Item::Mod(_)) {
                CapabilityConditionalVisitor {
                    inputs: self,
                    source,
                    ancestors,
                    inline_modules: Vec::new(),
                    implementation_attrs: None,
                    diagnostics,
                }
                .visit_item(item);
            }
            let syn::Item::Mod(module) = item else {
                continue;
            };
            let name = module.ident.unraw().to_string();
            let child_dir = if module_dir.is_empty() {
                name
            } else {
                format!("{module_dir}/{name}")
            };
            ancestors.push((source, module));
            if let Some((_, items)) = &module.content {
                self.validate_items(source, items, &child_dir, ancestors, diagnostics);
            } else {
                let target = self
                    .find(&format!("{child_dir}.rs"))
                    .or_else(|| self.find(&format!("{child_dir}/mod.rs")))
                    .expect("structural validation guarantees one outline target");
                self.validate_items(
                    target,
                    &self.inputs[target].syntax.items,
                    &child_dir,
                    ancestors,
                    diagnostics,
                );
            }
            ancestors.pop();
        }
    }

    fn validate_context(
        &self,
        source: usize,
        attributes: &[syn::Attribute],
        ancestors: &[(usize, &syn::ItemMod)],
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        self.add_conditionals(source, attributes, diagnostics);
        for &(source, module) in ancestors {
            self.add_conditionals(source, &module.attrs, diagnostics);
        }
    }

    fn add_conditionals(
        &self,
        source: usize,
        attributes: &[syn::Attribute],
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        attributes
            .iter()
            .filter_map(|attribute| conditional(attribute).map(|name| (attribute, name)))
            .for_each(|(attribute, offending)| {
                diagnostics.push(module_diagnostic(
                    &self.inputs[source].path,
                    attribute_span(attribute, false),
                    DiagnosticCode::Bxg0020,
                    offending,
                    CONDITIONAL_RULE,
                ));
            });
    }
}

fn valid_capability_call_shape(method: &syn::ImplItemFn) -> bool {
    let signature = &method.sig;
    let mut inputs = signature.inputs.iter();
    let receiver = inputs.next().and_then(|argument| match argument {
        syn::FnArg::Receiver(receiver) => Some(receiver),
        syn::FnArg::Typed(_) => None,
    });
    signature.asyncness.is_some()
        && receiver.is_some_and(|receiver| {
            matches!(receiver.kind, syn::ReceiverKind::Reference(_, _, None))
                && receiver.mutability.is_none()
        })
        && inputs.len() == 2
        && inputs.all(|argument| matches!(argument, syn::FnArg::Typed(_)))
        && signature.variadic.is_none()
        && matches!(signature.output, syn::ReturnType::Type(_, _))
}

fn parse_capability_metadata(
    declaration: &CapabilityDeclaration<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> CapabilityMarkerMetadata {
    let markers = declaration
        .method
        .attrs
        .iter()
        .filter(|attribute| {
            boxology_leaf(attribute).is_some_and(|leaf| leaf.unraw() == "capability")
        })
        .collect::<Vec<_>>();
    for marker in &markers[1..] {
        add_metadata_error(
            diagnostics,
            declaration,
            boxology_leaf(marker).unwrap().span(),
            DiagnosticCode::Bxg0032,
        );
    }
    let marker = markers[0];
    let mut draft = CapabilityMarkerMetadata {
        name_override: None,
        name_override_span: None,
        max_exposure: ExposureLevel::CodeOnly,
        idempotency: Idempotency::None,
    };
    let syn::Meta::List(list) = &marker.meta else {
        if !matches!(marker.meta, syn::Meta::Path(_)) {
            invalid_capability_marker(declaration, marker, diagnostics);
        }
        return draft;
    };
    let Ok(entries) = list.parse_args_with(
        syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
    ) else {
        invalid_capability_marker(declaration, marker, diagnostics);
        return draft;
    };
    if entries.is_empty() {
        invalid_capability_marker(declaration, marker, diagnostics);
        return draft;
    }
    let mut seen = BTreeSet::new();
    for entry in entries {
        let syn::Meta::NameValue(value) = entry else {
            invalid_capability_marker(declaration, marker, diagnostics);
            continue;
        };
        let Some(key) = value.path.get_ident() else {
            invalid_capability_marker(declaration, marker, diagnostics);
            continue;
        };
        let key_name = key.to_string();
        if !seen.insert(key_name.clone()) {
            add_metadata_error(
                diagnostics,
                declaration,
                key.span(),
                DiagnosticCode::Bxg0032,
            );
            continue;
        }
        if matches!(
            key_name.as_str(),
            "auth" | "default" | "min" | "max" | "validation"
        ) {
            add_metadata_error(
                diagnostics,
                declaration,
                key.span(),
                DiagnosticCode::Bxg0033,
            );
            continue;
        }
        if !matches!(key_name.as_str(), "name" | "exposure" | "idempotency") {
            add_metadata_error(
                diagnostics,
                declaration,
                key.span(),
                DiagnosticCode::Bxg0032,
            );
            continue;
        }
        let literal = match &value.value {
            syn::Expr::Lit(syn::ExprLit {
                attrs,
                lit: syn::Lit::Str(literal),
            }) if attrs.is_empty() => literal,
            _ => {
                invalid_capability_marker(declaration, marker, diagnostics);
                continue;
            }
        };
        match (key_name.as_str(), literal.value().as_str()) {
            ("name", _) => {
                draft.name_override = Some(literal.value());
                draft.name_override_span = Some(source_span(literal.span()));
            }
            ("exposure", "code-only") => draft.max_exposure = ExposureLevel::CodeOnly,
            ("exposure", "internal") => draft.max_exposure = ExposureLevel::Internal,
            ("exposure", "external") => draft.max_exposure = ExposureLevel::External,
            ("idempotency", "none") => draft.idempotency = Idempotency::None,
            ("idempotency", "inherent") => draft.idempotency = Idempotency::Inherent,
            ("idempotency", "keyed") => {
                add_metadata_error(
                    diagnostics,
                    declaration,
                    literal.span(),
                    DiagnosticCode::Bxg0033,
                );
            }
            ("exposure" | "idempotency", _) => {
                add_metadata_error(
                    diagnostics,
                    declaration,
                    literal.span(),
                    DiagnosticCode::Bxg0032,
                );
            }
            _ => unreachable!("supported keys are exhaustively matched"),
        }
    }
    draft
}

fn capability_identity_error(
    declaration: &CapabilityDeclaration<'_>,
    span: Span,
    code: DiagnosticCode,
) -> Diagnostic {
    let (offending, rule) = match code {
        DiagnosticCode::Bxg0034 => ("invalid effective capability name", CAPABILITY_NAME_RULE),
        DiagnosticCode::Bxg0035 => (
            "duplicate effective capability identity",
            CAPABILITY_IDENTITY_RULE,
        ),
        _ => unreachable!("capability identity diagnostics accept only BXG0034-BXG0035"),
    };
    Diagnostic {
        path: declaration.source.clone(),
        span,
        code,
        offending: offending.into(),
        rule,
        rule_source: CAPABILITY_IDENTITY_RULE_SOURCE,
    }
}

fn invalid_capability_marker(
    declaration: &CapabilityDeclaration<'_>,
    marker: &syn::Attribute,
    diagnostics: &mut Vec<Diagnostic>,
) {
    add_metadata_error(
        diagnostics,
        declaration,
        boxology_leaf(marker).unwrap().span(),
        DiagnosticCode::Bxg0032,
    );
}

fn add_metadata_error(
    diagnostics: &mut Vec<Diagnostic>,
    declaration: &CapabilityDeclaration<'_>,
    span: proc_macro2::Span,
    code: DiagnosticCode,
) {
    let offending = match code {
        DiagnosticCode::Bxg0032 => "invalid capability metadata",
        DiagnosticCode::Bxg0033 => "unsupported capability metadata",
        _ => unreachable!("metadata diagnostics accept only BXG0032-BXG0033"),
    };
    diagnostics.push(Diagnostic {
        path: declaration.source.clone(),
        span: source_span(span),
        code,
        offending: offending.into(),
        rule: CAPABILITY_METADATA_RULE,
        rule_source: CAPABILITY_METADATA_RULE_SOURCE,
    });
}

struct CapabilityCollector<'a, 'ast> {
    source: &'ast RelativePath,
    module_path: Vec<String>,
    implementation: Option<&'ast syn::ItemImpl>,
    declarations: &'a mut Vec<CapabilityDeclaration<'ast>>,
}

impl<'ast> Visit<'ast> for CapabilityCollector<'_, 'ast> {
    fn visit_attribute(&mut self, _: &'ast syn::Attribute) {}

    fn visit_item_mod(&mut self, module: &'ast syn::ItemMod) {
        if module.content.is_some() {
            self.module_path.push(module.ident.unraw().to_string());
            syn::visit::visit_item_mod(self, module);
            self.module_path.pop();
        }
    }

    fn visit_item_impl(&mut self, implementation: &'ast syn::ItemImpl) {
        let outer = self.implementation.replace(implementation);
        syn::visit::visit_item_impl(self, implementation);
        self.implementation = outer;
    }

    fn visit_impl_item_fn(&mut self, method: &'ast syn::ImplItemFn) {
        if let Some(implementation) = self.implementation
            && implementation.trait_.is_none()
            && has_boxology(&method.attrs, "capability")
        {
            self.declarations.push(CapabilityDeclaration {
                source: self.source,
                module_path: self.module_path.clone(),
                identifier_span: source_span(method.sig.ident.span()),
                implementation,
                method,
                marker_metadata: None,
                identity: None,
            });
        }
        syn::visit::visit_impl_item_fn(self, method);
    }
}

struct CapabilityConditionalVisitor<'a, 'ast> {
    inputs: &'a ParsedRustInputs,
    source: usize,
    ancestors: &'a [(usize, &'ast syn::ItemMod)],
    inline_modules: Vec<&'ast [syn::Attribute]>,
    implementation_attrs: Option<&'ast [syn::Attribute]>,
    diagnostics: &'a mut Vec<Diagnostic>,
}

impl<'ast> CapabilityConditionalVisitor<'_, 'ast> {
    fn inspect(&mut self, attributes: &'ast [syn::Attribute]) {
        if is_export(attributes) {
            self.inputs
                .validate_context(self.source, attributes, self.ancestors, self.diagnostics);
            if let Some(implementation) = self.implementation_attrs {
                self.inputs
                    .add_conditionals(self.source, implementation, self.diagnostics);
            }
            for attributes in &self.inline_modules {
                self.inputs
                    .add_conditionals(self.source, attributes, self.diagnostics);
            }
        }
    }
}

impl<'ast> Visit<'ast> for CapabilityConditionalVisitor<'_, 'ast> {
    fn visit_attribute(&mut self, _: &'ast syn::Attribute) {}

    fn visit_item_mod(&mut self, module: &'ast syn::ItemMod) {
        if let Some((_, items)) = &module.content {
            self.inline_modules.push(&module.attrs);
            items.iter().for_each(|item| self.visit_item(item));
            self.inline_modules.pop();
        }
    }

    fn visit_item_impl(&mut self, implementation: &'ast syn::ItemImpl) {
        let outer = self.implementation_attrs.replace(&implementation.attrs);
        syn::visit::visit_item_impl(self, implementation);
        self.implementation_attrs = outer;
    }

    fn visit_impl_item_fn(&mut self, method: &'ast syn::ImplItemFn) {
        self.inspect(&method.attrs);
        syn::visit::visit_impl_item_fn(self, method);
    }

    fn visit_item_fn(&mut self, function: &'ast syn::ItemFn) {
        self.inspect(&function.attrs);
        syn::visit::visit_item_fn(self, function);
    }

    fn visit_trait_item_fn(&mut self, method: &'ast syn::TraitItemFn) {
        self.inspect(&method.attrs);
        syn::visit::visit_trait_item_fn(self, method);
    }
}

struct UnreachableVisitor<'a> {
    path: &'a RelativePath,
    diagnostics: &'a mut Vec<Diagnostic>,
}

impl<'ast> Visit<'ast> for UnreachableVisitor<'_> {
    fn visit_attribute(&mut self, attribute: &'ast syn::Attribute) {
        if boxology_leaf(attribute).is_some() {
            self.diagnostics.push(module_diagnostic(
                self.path,
                attribute_span(attribute, true),
                DiagnosticCode::Bxg0019,
                "Boxology-annotated item",
                UNREACHABLE_RULE,
            ));
        }
    }

    fn visit_macro(&mut self, item: &'ast syn::Macro) {
        if exact_contract_macro(item) {
            self.diagnostics.push(module_diagnostic(
                self.path,
                item.path.segments[1].ident.span(),
                DiagnosticCode::Bxg0019,
                "Boxology contract invocation",
                UNREACHABLE_RULE,
            ));
        }
    }
}

struct ControlledSite<'a> {
    path: &'a RelativePath,
    item: &'a syn::ItemMacro,
}

fn collect_controlled_sites<'a>(
    path: &'a RelativePath,
    items: &'a [syn::Item],
    sites: &mut Vec<ControlledSite<'a>>,
) {
    for item in items {
        if let syn::Item::Macro(item) = item
            && exact_contract_macro(&item.mac)
        {
            sites.push(ControlledSite { path, item });
        }
        if let syn::Item::Mod(syn::ItemMod {
            content: Some((_, items)),
            ..
        }) = item
        {
            collect_controlled_sites(path, items, sites);
        }
    }
}

struct ControlledPlacementVisitor<'a> {
    path: &'a RelativePath,
    allowed: &'a BTreeSet<*const syn::Macro>,
    diagnostics: &'a mut Vec<Diagnostic>,
}

impl<'ast> Visit<'ast> for ControlledPlacementVisitor<'_> {
    fn visit_macro(&mut self, item: &'ast syn::Macro) {
        if exact_contract_macro(item) && !self.allowed.contains(&std::ptr::from_ref(item)) {
            self.diagnostics.push(module_diagnostic(
                self.path,
                item.path.segments[1].ident.span(),
                DiagnosticCode::Bxg0036,
                "misplaced controlled contract invocation",
                CONTROLLED_SITE_RULE,
            ));
        }
    }
}

fn exact_contract_macro(item: &syn::Macro) -> bool {
    item.path.leading_colon.is_none()
        && item.path.segments.len() == 2
        && item
            .path
            .segments
            .iter()
            .all(|segment| matches!(segment.arguments, syn::PathArguments::None))
        && item.path.segments[0].ident == "boxology"
        && item.path.segments[1].ident == "contract"
}

impl ParsedRustInput {
    /// Returns the exact validated logical input path.
    pub fn path(&self) -> &RelativePath {
        &self.path
    }

    /// Returns the complete parsed Rust syntax tree.
    pub fn syntax(&self) -> &syn::File {
        &self.syntax
    }
}

fn item_attrs(item: &syn::Item) -> &[syn::Attribute] {
    match item {
        syn::Item::Const(syn::ItemConst { attrs, .. })
        | syn::Item::Enum(syn::ItemEnum { attrs, .. })
        | syn::Item::ExternCrate(syn::ItemExternCrate { attrs, .. })
        | syn::Item::Fn(syn::ItemFn { attrs, .. })
        | syn::Item::ForeignMod(syn::ItemForeignMod { attrs, .. })
        | syn::Item::Impl(syn::ItemImpl { attrs, .. })
        | syn::Item::Macro(syn::ItemMacro { attrs, .. })
        | syn::Item::Mod(syn::ItemMod { attrs, .. })
        | syn::Item::Static(syn::ItemStatic { attrs, .. })
        | syn::Item::Struct(syn::ItemStruct { attrs, .. })
        | syn::Item::Trait(syn::ItemTrait { attrs, .. })
        | syn::Item::TraitAlias(syn::ItemTraitAlias { attrs, .. })
        | syn::Item::Type(syn::ItemType { attrs, .. })
        | syn::Item::Union(syn::ItemUnion { attrs, .. })
        | syn::Item::Use(syn::ItemUse { attrs, .. }) => attrs,
        _ => &[],
    }
}

fn boxology_leaf(attribute: &syn::Attribute) -> Option<&syn::Ident> {
    let path = attribute.path();
    if !matches!(&attribute.style, syn::AttrStyle::Outer)
        || path.segments.len() != 2
        || path
            .segments
            .iter()
            .any(|segment| !matches!(segment.arguments, syn::PathArguments::None))
        || path.segments[0].ident.unraw() != "boxology"
    {
        return None;
    }
    Some(&path.segments[1].ident)
}

fn has_boxology(attributes: &[syn::Attribute], leaf: &str) -> bool {
    attributes.iter().any(|attribute| {
        boxology_leaf(attribute).is_some_and(|identifier| identifier.unraw() == leaf)
    })
}

fn is_export(attributes: &[syn::Attribute]) -> bool {
    has_boxology(attributes, "contract") || has_boxology(attributes, "capability")
}

fn conditional(attribute: &syn::Attribute) -> Option<&'static str> {
    let path = attribute.path();
    if path.segments.len() != 1 || !matches!(path.segments[0].arguments, syn::PathArguments::None) {
        return None;
    }
    let identifier = path.segments[0].ident.unraw();
    (identifier == "cfg")
        .then_some("cfg attribute")
        .or_else(|| (identifier == "cfg_attr").then_some("cfg_attr attribute"))
}

fn attribute_span(attribute: &syn::Attribute, close_path: bool) -> proc_macro2::Span {
    let path = attribute.path();
    let start = path
        .leading_colon
        .as_ref()
        .map_or_else(|| path.segments[0].ident.span(), |colon| colon.spans[0]);
    let end = if close_path && matches!(&attribute.meta, syn::Meta::Path(_)) {
        attribute.bracket_token.span.close()
    } else {
        path.segments.last().unwrap().ident.span()
    };
    start.join(end).expect("attribute path spans one source")
}

fn validate_contract_attributes(
    declaration: &ContractDeclaration<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let path = declaration.source;
    match declaration.syntax {
        ContractDeclarationSyntax::Struct(item) => {
            validate_attributes(path, &item.attrs, diagnostics);
            for field in &item.fields {
                validate_attributes(path, &field.attrs, diagnostics);
            }
        }
        ContractDeclarationSyntax::Enum(item) => {
            validate_attributes(path, &item.attrs, diagnostics);
            for variant in &item.variants {
                validate_attributes(path, &variant.attrs, diagnostics);
                for field in &variant.fields {
                    validate_attributes(path, &field.attrs, diagnostics);
                }
            }
        }
    }
}

fn validate_contract_role(
    declaration: &mut ContractDeclaration<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let attributes = match declaration.syntax {
        ContractDeclarationSyntax::Struct(item) => &item.attrs,
        ContractDeclarationSyntax::Enum(item) => &item.attrs,
    };
    let mut contracts = attributes.iter().filter(|attribute| {
        boxology_leaf(attribute).is_some_and(|identifier| identifier.unraw() == "contract")
    });
    let owner = contracts
        .next()
        .expect("declaration discovery requires a direct contract attribute");
    let role = match &owner.meta {
        syn::Meta::Path(_) => Some(ContractDeclarationRole::Value),
        syn::Meta::List(list)
            if matches!(declaration.syntax, ContractDeclarationSyntax::Enum(_))
                && is_error_marker(list) =>
        {
            Some(ContractDeclarationRole::Error)
        }
        _ => None,
    };
    if let Some(role) = role {
        declaration.role = role;
    } else {
        diagnostics.push(contract_role_diagnostic(declaration.source, owner));
    }
    diagnostics
        .extend(contracts.map(|attribute| contract_role_diagnostic(declaration.source, attribute)));
}

fn is_error_marker(list: &syn::MetaList) -> bool {
    list.parse_args_with(syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated)
        .ok()
        .filter(|markers| markers.len() == 1)
        .is_some_and(|markers| {
            let marker = &markers[0];
            marker.leading_colon.is_none()
                && marker.segments.len() == 1
                && matches!(marker.segments[0].arguments, syn::PathArguments::None)
                && marker.segments[0].ident.unraw() == "error"
        })
}

fn contract_role_diagnostic(path: &RelativePath, attribute: &syn::Attribute) -> Diagnostic {
    Diagnostic {
        path: path.clone(),
        span: source_span(
            boxology_leaf(attribute)
                .expect("contract attributes are direct Boxology paths")
                .span(),
        ),
        code: DiagnosticCode::Bxg0024,
        offending: "invalid contract declaration annotation".into(),
        rule: CONTRACT_ROLE_RULE,
        rule_source: CONTRACT_ROLE_RULE_SOURCE,
    }
}

fn validate_contract_placement(
    reachable: &[&ParsedRustInput],
    declarations: &[ContractDeclaration<'_>],
    diagnostics: &mut Vec<Diagnostic>,
) {
    let allowed = declarations
        .iter()
        .flat_map(|declaration| match declaration.syntax {
            ContractDeclarationSyntax::Struct(item) => item.attrs.iter(),
            ContractDeclarationSyntax::Enum(item) => item.attrs.iter(),
        })
        .filter(|attribute| boxology_leaf(attribute).is_some_and(|leaf| leaf.unraw() == "contract"))
        .map(std::ptr::from_ref)
        .collect::<BTreeSet<_>>();
    for input in reachable {
        PlacementVisitor {
            path: &input.path,
            allowed: &allowed,
            diagnostics,
            leaf: "contract",
            code: DiagnosticCode::Bxg0029,
            offending: "misplaced contract declaration annotation",
            rule: CONTRACT_PLACEMENT_RULE,
            rule_source: CONTRACT_PLACEMENT_RULE_SOURCE,
        }
        .visit_file(&input.syntax);
    }
}

fn validate_capability_placement(
    reachable: &[&ParsedRustInput],
    declarations: &[CapabilityDeclaration<'_>],
    diagnostics: &mut Vec<Diagnostic>,
) {
    let allowed = declarations
        .iter()
        .flat_map(|declaration| declaration.method.attrs.iter())
        .filter(|attribute| {
            boxology_leaf(attribute).is_some_and(|leaf| leaf.unraw() == "capability")
        })
        .map(std::ptr::from_ref)
        .collect::<BTreeSet<_>>();
    for input in reachable {
        PlacementVisitor {
            path: &input.path,
            allowed: &allowed,
            diagnostics,
            leaf: "capability",
            code: DiagnosticCode::Bxg0030,
            offending: "misplaced capability annotation",
            rule: CAPABILITY_PLACEMENT_RULE,
            rule_source: CAPABILITY_PLACEMENT_RULE_SOURCE,
        }
        .visit_file(&input.syntax);
    }
}

struct PlacementVisitor<'a> {
    path: &'a RelativePath,
    allowed: &'a BTreeSet<*const syn::Attribute>,
    diagnostics: &'a mut Vec<Diagnostic>,
    leaf: &'static str,
    code: DiagnosticCode,
    offending: &'static str,
    rule: &'static str,
    rule_source: &'static str,
}

impl<'ast> Visit<'ast> for PlacementVisitor<'_> {
    fn visit_attribute(&mut self, attribute: &'ast syn::Attribute) {
        if let Some(leaf) = boxology_leaf(attribute)
            && leaf.unraw() == self.leaf
            && !self.allowed.contains(&std::ptr::from_ref(attribute))
        {
            self.diagnostics.push(Diagnostic {
                path: self.path.clone(),
                span: source_span(leaf.span()),
                code: self.code,
                offending: self.offending.into(),
                rule: self.rule,
                rule_source: self.rule_source,
            });
        }
    }
}

fn validate_contract_deprecations(
    declaration: &ContractDeclaration<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let path = declaration.source;
    match declaration.syntax {
        ContractDeclarationSyntax::Struct(item) => {
            validate_deprecations(path, &item.attrs, diagnostics);
            for field in &item.fields {
                validate_deprecations(path, &field.attrs, diagnostics);
            }
        }
        ContractDeclarationSyntax::Enum(item) => {
            validate_deprecations(path, &item.attrs, diagnostics);
            for variant in &item.variants {
                validate_deprecations(path, &variant.attrs, diagnostics);
                for field in &variant.fields {
                    validate_deprecations(path, &field.attrs, diagnostics);
                }
            }
        }
    }
}

fn validate_contract_documentation(
    declaration: &ContractDeclaration<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let path = declaration.source;
    match declaration.syntax {
        ContractDeclarationSyntax::Struct(item) => {
            validate_documentation(path, &item.attrs, diagnostics);
            for field in &item.fields {
                validate_documentation(path, &field.attrs, diagnostics);
            }
        }
        ContractDeclarationSyntax::Enum(item) => {
            validate_documentation(path, &item.attrs, diagnostics);
            for variant in &item.variants {
                validate_documentation(path, &variant.attrs, diagnostics);
                for field in &variant.fields {
                    validate_documentation(path, &field.attrs, diagnostics);
                }
            }
        }
    }
}

fn validate_member_identities(
    declaration: &ContractDeclaration<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match declaration.syntax {
        ContractDeclarationSyntax::Struct(item) => {
            validate_field_identities(declaration.source, &item.fields, diagnostics);
        }
        ContractDeclarationSyntax::Enum(item) => {
            let mut variants = BTreeSet::new();
            for variant in &item.variants {
                if !variants.insert(member_identity_name(&variant.ident)) {
                    diagnostics.push(Diagnostic {
                        path: declaration.source.clone(),
                        span: source_span(variant.ident.span()),
                        code: DiagnosticCode::Bxg0028,
                        offending: "duplicate contract enum variant identity".into(),
                        rule: VARIANT_IDENTITY_RULE,
                        rule_source: MEMBER_IDENTITY_RULE_SOURCE,
                    });
                }
                validate_field_identities(declaration.source, &variant.fields, diagnostics);
            }
        }
    }
}

fn validate_field_identities(
    path: &RelativePath,
    fields: &syn::Fields,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let syn::Fields::Named(fields) = fields else {
        return;
    };
    let mut identities = BTreeSet::new();
    for field in &fields.named {
        let identifier = field
            .ident
            .as_ref()
            .expect("named fields always carry identifiers");
        if !identities.insert(member_identity_name(identifier)) {
            diagnostics.push(Diagnostic {
                path: path.clone(),
                span: source_span(identifier.span()),
                code: DiagnosticCode::Bxg0027,
                offending: "duplicate named contract field identity".into(),
                rule: FIELD_IDENTITY_RULE,
                rule_source: MEMBER_IDENTITY_RULE_SOURCE,
            });
        }
    }
}

fn project_declaration<'ast>(
    syntax: ContractDeclarationSyntax<'ast>,
) -> (ContractDeclarationShape<'ast>, ContractSiteMetadata) {
    let (shape, metadata) = match syntax {
        ContractDeclarationSyntax::Struct(item) => (
            ContractDeclarationShape::Struct(project_fields(&item.fields)),
            project_metadata(&item.attrs),
        ),
        ContractDeclarationSyntax::Enum(item) => (
            ContractDeclarationShape::Enum(
                item.variants
                    .iter()
                    .enumerate()
                    .map(|(ordinal, variant)| ContractVariant {
                        ordinal,
                        identity: member_identity(&variant.ident),
                        syntax: variant,
                        fields: project_fields(&variant.fields),
                        metadata: project_metadata(&variant.attrs),
                    })
                    .collect(),
            ),
            project_metadata(&item.attrs),
        ),
    };
    (shape, metadata)
}

fn project_fields<'ast>(fields: &'ast syn::Fields) -> ContractFields<'ast> {
    let mut project = |(ordinal, field): (usize, &'ast syn::Field)| -> ContractField<'ast> {
        ContractField {
            ordinal,
            identity: field.ident.as_ref().map(member_identity),
            syntax: field,
            metadata: project_metadata(&field.attrs),
        }
    };
    match fields {
        syn::Fields::Named(fields) => {
            ContractFields::Named(fields.named.iter().enumerate().map(&mut project).collect())
        }
        syn::Fields::Unnamed(fields) => {
            ContractFields::Unnamed(fields.unnamed.iter().enumerate().map(project).collect())
        }
        syn::Fields::Unit => ContractFields::Unit,
    }
}

fn member_identity(identifier: &syn::Ident) -> ContractMemberIdentity<'_> {
    ContractMemberIdentity {
        name: member_identity_name(identifier),
        ident: identifier,
    }
}

fn member_identity_name(identifier: &syn::Ident) -> String {
    identifier.unraw().to_string()
}

fn project_metadata(attributes: &[syn::Attribute]) -> ContractSiteMetadata {
    let deprecation = attributes
        .iter()
        .find(|attribute| {
            matches!(attribute.style, syn::AttrStyle::Outer)
                && attribute
                    .path()
                    .get_ident()
                    .is_some_and(|identifier| identifier.unraw() == "deprecated")
        })
        .filter(|attribute| valid_deprecation(attribute))
        .map(|attribute| ContractDeprecation {
            note: if let syn::Meta::List(list) = &attribute.meta {
                list.parse_args_with(
                    syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
                )
                .ok()
                .and_then(|entries| match &entries[0] {
                    syn::Meta::NameValue(value) => match &value.value {
                        syn::Expr::Lit(syn::ExprLit {
                            lit: syn::Lit::Str(note),
                            ..
                        }) => Some(note.value()),
                        _ => None,
                    },
                    _ => None,
                })
            } else {
                None
            },
        });
    let docs = attributes.iter().filter_map(documentation).collect();
    ContractSiteMetadata { deprecation, docs }
}

fn documentation_identifier(attribute: &syn::Attribute) -> Option<&syn::Ident> {
    let path = attribute.path();
    if !matches!(attribute.style, syn::AttrStyle::Outer)
        || path.leading_colon.is_some()
        || path.segments.len() != 1
        || !matches!(path.segments[0].arguments, syn::PathArguments::None)
    {
        return None;
    }
    let identifier = &path.segments[0].ident;
    (identifier.unraw() == "doc").then_some(identifier)
}

fn documentation(attribute: &syn::Attribute) -> Option<String> {
    documentation_identifier(attribute)?;
    let syn::Meta::NameValue(value) = &attribute.meta else {
        return None;
    };
    match &value.value {
        syn::Expr::Lit(syn::ExprLit {
            attrs,
            lit: syn::Lit::Str(value),
            ..
        }) if attrs.is_empty() => Some(value.value()),
        _ => None,
    }
}

fn validate_documentation(
    path: &RelativePath,
    attributes: &[syn::Attribute],
    diagnostics: &mut Vec<Diagnostic>,
) {
    for attribute in attributes {
        if let Some(identifier) = documentation_identifier(attribute)
            && documentation(attribute).is_none()
        {
            diagnostics.push(Diagnostic {
                path: path.clone(),
                span: source_span(identifier.span()),
                code: DiagnosticCode::Bxg0026,
                offending: "invalid documentation attribute".into(),
                rule: DOCUMENTATION_RULE,
                rule_source: DOCUMENTATION_RULE_SOURCE,
            });
        }
    }
}

fn validate_deprecations(
    path: &RelativePath,
    attributes: &[syn::Attribute],
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut deprecations = attributes.iter().filter(|attribute| {
        matches!(attribute.style, syn::AttrStyle::Outer)
            && attribute
                .path()
                .get_ident()
                .is_some_and(|identifier| identifier.unraw() == "deprecated")
    });
    let Some(owner) = deprecations.next() else {
        return;
    };
    if !valid_deprecation(owner) {
        diagnostics.push(deprecation_diagnostic(path, owner));
    }
    diagnostics.extend(deprecations.map(|attribute| deprecation_diagnostic(path, attribute)));
}

fn valid_deprecation(attribute: &syn::Attribute) -> bool {
    let syn::Meta::List(list) = &attribute.meta else {
        return matches!(&attribute.meta, syn::Meta::Path(_));
    };
    let Ok(entries) = list.parse_args_with(
        syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
    ) else {
        return false;
    };
    let Some(syn::Meta::NameValue(note)) = entries.first().filter(|_| entries.len() == 1) else {
        return false;
    };
    note.path
        .get_ident()
        .is_some_and(|identifier| identifier.unraw() == "note")
        && matches!(
            &note.value,
            syn::Expr::Lit(syn::ExprLit {
                attrs,
                lit: syn::Lit::Str(_),
                ..
            }) if attrs.is_empty()
        )
}

fn deprecation_diagnostic(path: &RelativePath, attribute: &syn::Attribute) -> Diagnostic {
    Diagnostic {
        path: path.clone(),
        span: source_span(
            attribute
                .path()
                .get_ident()
                .expect("deprecation validation owns direct attributes")
                .span(),
        ),
        code: DiagnosticCode::Bxg0025,
        offending: "invalid or duplicate deprecation attribute".into(),
        rule: DEPRECATION_RULE,
        rule_source: DEPRECATION_RULE_SOURCE,
    }
}

fn validate_attributes(
    path: &RelativePath,
    attributes: &[syn::Attribute],
    diagnostics: &mut Vec<Diagnostic>,
) {
    for attribute in attributes {
        if boxology_leaf(attribute).is_some() {
            continue;
        }
        let name = attribute
            .path()
            .get_ident()
            .filter(|_| matches!(attribute.style, syn::AttrStyle::Outer))
            .map(|identifier| identifier.unraw().to_string());
        match name.as_deref() {
            Some("doc" | "deprecated") => {}
            Some("derive") => validate_derives(path, attribute, diagnostics),
            _ => diagnostics.push(module_diagnostic(
                path,
                attribute_span(attribute, false),
                DiagnosticCode::Bxg0022,
                "non-allowlisted contract attribute",
                ATTRIBUTE_RULE,
            )),
        }
    }
}

fn validate_derives(
    path: &RelativePath,
    attribute: &syn::Attribute,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Ok(derives) = attribute.parse_args_with(
        syn::punctuated::Punctuated::<ParsedDerive, syn::Token![,]>::parse_terminated,
    ) else {
        diagnostics.push(module_diagnostic(
            path,
            attribute_span(attribute, false),
            DiagnosticCode::Bxg0023,
            "non-allowlisted contract derive",
            DERIVE_RULE,
        ));
        return;
    };
    for derive in derives {
        let allowed = derive
            .path
            .get_ident()
            .map(syn::Ident::unraw)
            .is_some_and(|name| name == "Debug" || name == "Clone" || name == "PartialEq");
        if !allowed {
            diagnostics.push(module_diagnostic(
                path,
                derive.span,
                DiagnosticCode::Bxg0023,
                "non-allowlisted contract derive",
                DERIVE_RULE,
            ));
        }
    }
}

struct ParsedDerive {
    path: syn::Path,
    span: proc_macro2::Span,
}

impl syn::parse::Parse for ParsedDerive {
    fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
        let start = input.cursor();
        let mut path: syn::Path = input.parse()?;
        if input.peek(syn::token::Paren) {
            path.segments.last_mut().unwrap().arguments =
                syn::PathArguments::Parenthesized(input.parse()?);
        }
        let finish = input.cursor();
        let (first, mut cursor) = start.token_tree().expect("a parsed path has tokens");
        let mut last = first.span();
        while cursor != finish {
            let (token, next) = cursor
                .token_tree()
                .expect("finish follows parsed path tokens");
            last = token.span();
            cursor = next;
        }
        Ok(Self {
            path,
            span: first
                .span()
                .join(last)
                .expect("derive path spans one source"),
        })
    }
}

fn append_errors(path: &RelativePath, error: syn::Error, diagnostics: &mut Vec<Diagnostic>) {
    for component in error {
        diagnostics.push(Diagnostic {
            path: path.clone(),
            span: source_span(component.span()),
            code: DiagnosticCode::Bxg0014,
            offending: "Rust source syntax".into(),
            rule: RULE,
            rule_source: RULE_SOURCE,
        });
    }
}

fn module_diagnostic(
    path: &RelativePath,
    span: proc_macro2::Span,
    code: DiagnosticCode,
    offending: &'static str,
    rule: &'static str,
) -> Diagnostic {
    Diagnostic {
        path: path.clone(),
        span: source_span(span),
        code,
        offending: offending.into(),
        rule,
        rule_source: RULE_SOURCE,
    }
}

fn source_span(upstream: proc_macro2::Span) -> Span {
    let (start, end) = (upstream.start(), upstream.end());
    Span {
        start: LineColumn {
            line: start.line,
            column: start.column + 1,
        },
        end: LineColumn {
            line: end.line,
            column: end.column + 1,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use boxology_contract::BoxId;
    use syn::parse::Parser as _;

    fn request(root: &str, files: &[(&str, &str)]) -> GenerationRequest {
        request_in_order(root, files, false)
    }

    fn request_in_order(root: &str, files: &[(&str, &str)], reversed: bool) -> GenerationRequest {
        let mut inputs = vec![(
            "boxology.toml".into(),
            b"schema = 1\nid = \"demo\"\nkind = \"box\"\n".to_vec(),
        )];
        inputs.extend(
            files
                .iter()
                .map(|(path, source)| ((*path).into(), source.as_bytes().to_vec())),
        );
        if reversed {
            inputs.reverse();
        }
        GenerationRequest::new(
            BoxId::new("demo").unwrap(),
            root.into(),
            inputs,
            vec![],
            vec![],
        )
        .unwrap()
    }

    fn parse_errors(request: &GenerationRequest) -> Diagnostics {
        match ParsedRustInputs::parse(request) {
            Ok(_) => panic!("expected Rust syntax diagnostics"),
            Err(diagnostics) => diagnostics,
        }
    }

    fn resolution_errors(request: &GenerationRequest) -> Diagnostics {
        let parsed = ParsedRustInputs::parse(request).unwrap();
        match parsed.resolve_reachable_inputs() {
            Ok(_) => panic!("expected Rust module diagnostics"),
            Err(diagnostics) => diagnostics,
        }
    }

    fn discovery_errors(request: &GenerationRequest) -> Diagnostics {
        let parsed = ParsedRustInputs::parse(request).unwrap();
        match parsed.discover_contract_declarations() {
            Ok(_) => panic!("expected contract declaration diagnostics"),
            Err(diagnostics) => diagnostics,
        }
    }

    fn capability_errors(request: &GenerationRequest) -> Diagnostics {
        let parsed = ParsedRustInputs::parse(request).unwrap();
        parsed
            .discover_capability_declarations()
            .err()
            .expect("expected capability declaration diagnostics")
    }

    fn capability_shape_errors(request: &GenerationRequest) -> Diagnostics {
        let parsed = ParsedRustInputs::parse(request).unwrap();
        parsed
            .validate_capability_call_shapes()
            .err()
            .expect("expected capability call-shape diagnostics")
    }

    fn capability_metadata_errors(request: &GenerationRequest) -> Diagnostics {
        ParsedRustInputs::parse(request)
            .unwrap()
            .validate_capability_marker_metadata()
            .err()
            .expect("expected capability metadata diagnostics")
    }

    fn capability_identity_errors(request: &GenerationRequest) -> Diagnostics {
        ParsedRustInputs::parse(request)
            .unwrap()
            .validate_capability_identities()
            .err()
            .expect("expected capability identity diagnostics")
    }

    #[test]
    fn value_payloads_are_emittable_and_named_payloads_remain_fail_closed() {
        let value = "boxology::contract! { #[error] pub enum Fault { Code(u32) } #[capability(exposure=external)] pub async fn greet(name:String)->Result<String,Fault>; }";
        let model = ParsedRustInputs::parse(&request("root.rs", &[("root.rs", value)]))
            .unwrap()
            .controlled_contract()
            .unwrap();
        model
            .require_v0_emittable()
            .expect("one-value payloads must gate clean");

        let named = "boxology::contract! { #[error] pub enum Fault { Detail { message: String } } #[capability(exposure=external)] pub async fn greet(name:String)->Result<String,Fault>; }";
        let model = ParsedRustInputs::parse(&request("root.rs", &[("root.rs", named)]))
            .unwrap()
            .controlled_contract()
            .unwrap();
        let diagnostics = model.require_v0_emittable().unwrap_err();
        assert_eq!(
            diagnostics.to_string(),
            "BXG0048 root.rs:1:11-1:19 offending=\"named-field error variants are not yet emittable\" rule=\"named-field payloads require contract-emitter support\" source=\"specs/s2-contract-generator.md D3\""
        );

        let value_blob = "boxology::contract! { #[error] pub enum Fault { Code(Blob) } #[capability(exposure=external)] pub async fn greet(name:String)->Result<String,Fault>; }";
        let blob = ParsedRustInputs::parse(&request("root.rs", &[("root.rs", value_blob)]))
            .unwrap()
            .controlled_contract()
            .unwrap();
        let diagnostics = blob.require_v0_emittable().unwrap_err();
        assert_eq!(
            diagnostics.to_string(),
            "BXG0040 root.rs:1:11-1:19 offending=\"Blob capability boundary or value-payload leaf not yet emittable in v0\" rule=\"the `Blob` capability boundary or value-payload leaf is parsed and modelled but its v0 end-to-end runtime generation is not yet implemented (deferred); scalar leaves and `String` are emittable.\" source=\"specs/s2-contract-generator.md D3,D5\""
        );

        let empty_named = "boxology::contract! { #[error] pub enum Fault { EmptyNamed {} } #[capability(exposure=external)] pub async fn greet(name:String)->Result<String,Fault>; }";
        let empty = ParsedRustInputs::parse(&request("root.rs", &[("root.rs", empty_named)]))
            .unwrap()
            .controlled_contract()
            .unwrap();
        let diagnostics = empty.require_v0_emittable().unwrap_err();
        assert_eq!(diagnostics.as_slice().len(), 1);
        assert_eq!(diagnostics.as_slice()[0].code(), "BXG0048");

        let named_and_blob_boundary = "boxology::contract! { #[error] pub enum Fault { Detail { message: String } } #[capability(exposure=external)] pub async fn greet(name:Blob)->Result<String,Fault>; }";
        let mixed =
            ParsedRustInputs::parse(&request("root.rs", &[("root.rs", named_and_blob_boundary)]))
                .unwrap()
                .controlled_contract()
                .unwrap();
        let diagnostics = mixed.require_v0_emittable().unwrap_err();
        assert_eq!(
            diagnostics
                .as_slice()
                .iter()
                .map(|diagnostic| diagnostic.code())
                .collect::<Vec<_>>(),
            ["BXG0040", "BXG0048"]
        );

        let named_blob = "boxology::contract! { #[error] pub enum Fault { Detail { message: Blob } } #[capability(exposure=external)] pub async fn greet(name:String)->Result<String,Fault>; }";
        let payload_blob = ParsedRustInputs::parse(&request("root.rs", &[("root.rs", named_blob)]))
            .unwrap()
            .controlled_contract()
            .unwrap();
        let diagnostics = payload_blob.require_v0_emittable().unwrap_err();
        assert_eq!(diagnostics.as_slice().len(), 1);
        assert_eq!(diagnostics.as_slice()[0].code(), "BXG0048");

        let invalid = named.replace("message: String", "message: Vec<u8>");
        let diagnostics = ParsedRustInputs::parse(&request("root.rs", &[("root.rs", &invalid)]))
            .unwrap()
            .controlled_contract()
            .err()
            .expect("payload syntax must fail through the controlled parser");
        let diagnostic = &diagnostics.as_slice()[0];
        assert_eq!(diagnostic.code(), "BXG0038");
        assert_eq!(
            &invalid[diagnostic.span().start().column() - 1..diagnostic.span().end().column() - 1],
            "Vec<u8>"
        );
    }

    #[test]
    #[rustfmt::skip]
    fn structured_contract_grammar_and_residual_emitter_gates_are_exact() {
        const VALID: &str = "boxology::contract! { pub struct A { pub x: String } pub enum Kind { One } #[error] pub enum Fault { Bad } #[capability] pub async fn go(input:A)->Result<Option<Vec<A>>,Fault>; }";
        let parsed = ParsedRustInputs::parse(&request("root.rs", &[("root.rs", VALID)])).unwrap();
        let model = parsed.controlled_contract().unwrap();
        assert_eq!(model.model().data.len(), 2);
        model.require_v0_emittable().expect("the narrow structured subset is emittable");
        let mutations = [
            ("pub struct A { pub x: String }", "struct A { pub x: String }", "struct A { pub x: String }"),
            ("{ pub x: String }", "(pub String);", "(pub String)"), ("{ pub x: String }", ";", "A"),
            ("pub x: String", "x: String", "x"), ("One", "One(String)", "One(String)"),
            ("{ One }", "{}", "pub enum Kind {}"), ("One", "One = 1", "One = 1"), ("One", "Unknown", "Unknown"),
            ("pub enum Kind", "pub struct A {} pub enum Kind", "A"),
            ("pub x: String", "pub x: String, pub x: u8", "x"), ("One", "One, One", "One"),
            ("x: String", "x: A", "A"), ("x: String", "x: Kind", "Kind"),
            ("x: String", "x: Missing", "Missing"), ("x: String", "x: Fault", "Fault"),
            ("x: String", "x: crate::A", "crate::A"), ("x: String", "x: foreign::A", "foreign::A"),
            ("pub struct A { pub x: String }", "pub type A = String;", "pub"),
            ("String", "(String,String)", "(String,String)"), ("String", "&String", "&String"),
            ("String", "BTreeMap<String,String>", "BTreeMap<String,String>"),
            ("String", "Field<String>", "Field<String>"), ("String", "Secret<String>", "Secret<String>"),
            ("String", "Box<String>", "Box<String>"), ("String", "Option<String,u8>", "Option<String,u8>"),
            ("String", "Vec<String,u8>", "Vec<String,u8>"), ("String", "Option<>", "Option<>"),
            ("String", "option<String>", "option<String>"), ("String", "Option<Option<String>>", "Option<String>"),
            ("String", "Vec<Option<String>>", "Option<String>"), ("String", "Vec<Vec<String>>", "Vec<String>"),
            ("String", "Option<Vec<Vec<String>>>", "Vec<String>"),
            ("struct A", "struct r#A", "r#A"), ("pub x", "pub r#x", "r#x"), ("One", "r#One", "r#One"),
        ];
        for (from, to, expected) in mutations {
            let source = VALID.replacen(from, to, 1);
            let failure = ParsedRustInputs::parse(&request("root.rs", &[("root.rs", &source)])).unwrap().controlled_contract().err().expect("mutant must be rejected");
            assert_eq!(failure.as_slice().len(), 1, "{to}");
            let diagnostic = &failure.as_slice()[0];
            assert_eq!(diagnostic.code(), "BXG0038", "{to}");
            assert_eq!(&source[diagnostic.span().start().column() - 1..diagnostic.span().end().column() - 1], expected, "{to}");
        }
        for leaf in ["bool", "u8", "u16", "u32", "u64", "i8", "i16", "i32", "i64", "f32", "f64", "String", "Blob"] {
            let source = VALID.replacen("struct A", &format!("struct {leaf}"), 1);
            let failure = ParsedRustInputs::parse(&request("root.rs", &[("root.rs", &source)])).unwrap().controlled_contract().err().expect("leaf-shadow mutant must be rejected");
            let diagnostic = &failure.as_slice()[0];
            assert_eq!((diagnostic.code(), &source[diagnostic.span().start().column() - 1..diagnostic.span().end().column() - 1]), ("BXG0038", leaf));
        }
        let blob = VALID.replace("Option<Vec<A>>", "Option<Vec<Blob>>");
        let model = ParsedRustInputs::parse(&request("root.rs", &[("root.rs", &blob)])).unwrap().controlled_contract().unwrap();
        assert_eq!(model.require_v0_emittable().unwrap_err().as_slice().iter().map(|item| item.code()).collect::<Vec<_>>(), ["BXG0040"]);
    }

    fn assert_metadata_error(attributes: &str, code: &str, expected_span: &str) {
        let source = format!(
            "struct H; impl H {{ {attributes} async fn good(&self, a: A, b: B) -> R {{ loop {{}} }} }}"
        );
        let diagnostics = capability_metadata_errors(&request("root.rs", &[("root.rs", &source)]));
        assert_eq!(diagnostics.as_slice().len(), 1);
        let diagnostic = &diagnostics.as_slice()[0];
        assert_eq!(diagnostic.code(), code);
        let range = diagnostic.span();
        assert_eq!(
            &source[range.start().column() - 1..range.end().column() - 1],
            expected_span
        );
        assert_eq!(diagnostic.rule_source(), CAPABILITY_METADATA_RULE_SOURCE);
        assert!(!format!("{diagnostics}\n{diagnostics:?}").contains("DO_NOT_LEAK"));
    }

    fn span(start: (usize, usize), end: (usize, usize)) -> Span {
        Span {
            start: LineColumn {
                line: start.0,
                column: start.1,
            },
            end: LineColumn {
                line: end.0,
                column: end.1,
            },
        }
    }

    #[test]
    fn valid_inputs_are_byte_sorted_and_retain_inspectable_files() {
        let request = request(
            "a.rs",
            &[("z.rs", "fn z() {}\n"), ("a.rs", "struct A;\nfn a() {}\n")],
        );
        let parsed = ParsedRustInputs::parse(&request).unwrap();
        assert_eq!(
            parsed
                .as_slice()
                .iter()
                .map(|input| (input.path().as_str(), input.syntax().items.len()))
                .collect::<Vec<_>>(),
            [("a.rs", 2), ("z.rs", 1)]
        );
        assert!(matches!(
            parsed.as_slice()[0].syntax().items[0],
            syn::Item::Struct(_)
        ));
    }

    #[test]
    fn resolves_structural_modules_from_a_nonstandard_root() {
        let request = request(
            "source/custom-entry.rs",
            &[
                ("source/unreachable.rs", "mod also_unreachable;\n"),
                ("source/type.rs", ""),
                ("source/flat/child.rs", ""),
                (
                    "source/custom-entry.rs",
                    "mod flat;\nmod directory;\nmod inline { mod deeper { mod leaf; } }\nmod r#type;\n",
                ),
                ("source/directory/mod.rs", "mod child;\n"),
                ("source/inline/deeper/leaf.rs", ""),
                ("source/flat.rs", "mod child;\n"),
                ("source/directory/child.rs", ""),
            ],
        );
        let parsed = ParsedRustInputs::parse(&request).unwrap();
        assert!(
            parsed
                .as_slice()
                .iter()
                .any(|input| input.path().as_str() == "source/unreachable.rs")
        );
        let paths = parsed
            .resolve_reachable_inputs()
            .unwrap()
            .iter()
            .map(|input| input.path().as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            paths,
            [
                "source/custom-entry.rs",
                "source/directory/child.rs",
                "source/directory/mod.rs",
                "source/flat.rs",
                "source/flat/child.rs",
                "source/inline/deeper/leaf.rs",
                "source/type.rs",
            ]
        );
        assert!(
            paths
                .windows(2)
                .all(|pair| pair[0].as_bytes() < pair[1].as_bytes())
        );
    }

    #[test]
    fn direct_boxology_paths_are_exact_and_each_marker_is_reported() {
        let expected = |line, end| {
            format!(
                "BXG0019 dead.rs:{line}:3-{line}:{end} offending={:?} rule={UNREACHABLE_RULE:?} source={RULE_SOURCE:?}",
                "Boxology-annotated item"
            )
        };
        let one = resolution_errors(&request(
            "root.rs",
            &[
                ("root.rs", ""),
                ("dead.rs", "#[boxology::contract] struct Dead;"),
            ],
        ));
        assert_eq!(one.to_string(), expected(1, 22));
        let source = "#[r#boxology::r#contract] struct Raw;\n#[::r#boxology::r#capability] fn leading() {}\n#[boxology::contract]\n#[::boxology::capability]\nfn twice() {}\n#[alias::contract] fn alias() {}\n#[crate::boxology::contract] fn prefixed() {}\n#[boxology] fn short() {}\n#[boxology::contract::nested] fn long() {}\n";
        let errors =
            resolution_errors(&request("root.rs", &[("root.rs", ""), ("dead.rs", source)]));
        let rendered = [(1, 26), (2, 30), (3, 22), (4, 26)]
            .map(|(line, end)| expected(line, end))
            .join("\n");
        assert_eq!(errors.to_string(), rendered);
    }

    #[test]
    fn conditional_export_item_spellings_have_exact_path_spans() {
        for (attribute, end, offending) in [
            ("#[cfg(payload)]", 6, "cfg attribute"),
            ("#[cfg_attr(payload, ignored)]", 11, "cfg_attr attribute"),
            ("#[r#cfg(payload)]", 8, "cfg attribute"),
            ("#[::r#cfg_attr(x, y)]", 15, "cfg_attr attribute"),
        ] {
            let source = format!("{attribute}\n#[boxology::contract]\nstruct Export;");
            let errors = resolution_errors(&request("root.rs", &[("root.rs", &source)]));
            assert_eq!(
                errors.to_string(),
                format!(
                    "BXG0020 root.rs:1:3-1:{end} offending={offending:?} rule={CONDITIONAL_RULE:?} source={RULE_SOURCE:?}"
                )
            );
        }
    }

    #[test]
    fn declaration_errors_are_complete_sorted_deduplicated_and_safe() {
        let request = request(
            "root.rs",
            &[
                ("a-dead.rs", "#[boxology::contract(payload)] struct Secret;"),
                (
                    "inline/child.rs",
                    "#[cfg(deep_payload)]\nmod deep {\n#[boxology::contract]\nstruct One;\n#[boxology::capability]\nfn two() {}\n}",
                ),
                (
                    "root.rs",
                    "#[boxology::contract]\nmod inline {\n#![cfg(root_payload)]\n#[cfg_attr(child_payload, ignored)]\nmod child;\n}\n#[boxology::contract]\nstruct S {\n#[cfg(field_payload)]\nfield: u8,\n}\n#[boxology::contract]\nenum E {\n#[cfg(variant_payload)]\nA,\nB {\n#[cfg_attr(field_payload, ignored)]\nfield: u8,\n}\n}\nstruct Plain;\nimpl Plain {\n#![cfg(impl_payload)]\n#[cfg_attr(method_payload, ignored)]\n#[boxology::capability]\nfn cap(&self) {}\n#[cfg(unrelated_payload)]\nfn helper(&self) {}\n}",
                ),
                (
                    "z-dead.rs",
                    "struct Z;\nimpl Z {\n#[boxology::contract]\n#[::boxology::capability]\nfn twice() {}\n}",
                ),
            ],
        );
        let (first, second) = (resolution_errors(&request), resolution_errors(&request));
        assert_eq!(first, second);
        let sites = [
            ("a-dead.rs", 1, 21),
            ("inline/child.rs", 1, 6),
            ("root.rs", 3, 7),
            ("root.rs", 4, 11),
            ("root.rs", 9, 6),
            ("root.rs", 14, 6),
            ("root.rs", 17, 11),
            ("root.rs", 23, 7),
            ("root.rs", 24, 11),
            ("z-dead.rs", 3, 22),
            ("z-dead.rs", 4, 26),
        ];
        let expected = sites.map(|(path, line, end)| {
            let start = if end == 7 { 4 } else { 3 };
            let (code, offending, rule) = match end {
                6 | 7 => ("BXG0020", "cfg attribute", CONDITIONAL_RULE),
                11 => ("BXG0020", "cfg_attr attribute", CONDITIONAL_RULE),
                _ => ("BXG0019", "Boxology-annotated item", UNREACHABLE_RULE),
            };
            format!("{code} {path}:{line}:{start}-{line}:{end} offending={offending:?} rule={rule:?} source={RULE_SOURCE:?}")
        }).join("\n");
        assert_eq!(first.to_string(), expected);
        let rendered = first.to_string();
        for payload in ["payload", "Secret", "One", "cap", "ignored"] {
            assert!(!rendered.contains(payload));
        }
    }

    #[test]
    fn conditionals_outside_export_shape_are_allowed() {
        let source = "#![cfg(file_payload)]\n#[cfg(internal_payload)] fn internal() {}\nstruct Plain { #[cfg(field_payload)] field: u8 }\nenum PlainEnum { #[cfg(variant_payload)] A, B { #[cfg_attr(field_payload, ignored)] field: u8 } }\n#[cfg(sibling_payload)] mod sibling {}\nimpl Plain { #[cfg(helper_payload)] fn helper(&self) {} #[boxology::capability] fn exported(&self) {} }";
        let valid = request("root.rs", &[("root.rs", source)]);
        let parsed = ParsedRustInputs::parse(&valid).unwrap();
        assert!(parsed.resolve_reachable_inputs().is_ok());
    }

    #[test]
    fn provisional_discovery_is_canonical_and_ignores_unresolved_macro_tokens() {
        let files = [
            (
                "src/root.rs",
                concat!(
                    "#[boxology::contract]\nstruct Foo;\nmod alpha;\nmod r#inline {\n",
                    "#[boxology::contract(error)]\nenum Fault { HiddenVariant }\n",
                    "#[boxology::contract]\nstruct r#foo;\nstruct Plain;\n",
                    "macro_rules! hidden { () => { #[boxology::contract] struct Macro; } }\n",
                    "struct Host;\nimpl Host { #[boxology::capability] fn cap(&self) {} }\n}\n",
                ),
            ),
            (
                "src/alpha.rs",
                "#[boxology::contract]\nenum Ordinary { Hidden }\nmod deep;\n",
            ),
            (
                "src/alpha/deep/mod.rs",
                "#[boxology::contract]\nstruct Deep;\n",
            ),
        ];
        let project = |files: &[(&str, &str)]| {
            let request = request("src/root.rs", files);
            let parsed = ParsedRustInputs::parse(&request).unwrap();
            parsed
                .discover_contract_declarations()
                .unwrap()
                .into_iter()
                .map(|declaration| {
                    let (kind, syntax_name) = match declaration.syntax() {
                        ContractDeclarationSyntax::Struct(item) => ("struct", item.ident.unraw()),
                        ContractDeclarationSyntax::Enum(item) => ("enum", item.ident.unraw()),
                    };
                    assert_eq!(syntax_name, declaration.lifted_name());
                    let span = declaration.identifier_span();
                    format!(
                        "{kind}|[{}]|{}|{}|{}:{}-{}:{}",
                        declaration.module_path().join("::"),
                        declaration.lifted_name(),
                        declaration.source().as_str().to_owned(),
                        span.start().line(),
                        span.start().column(),
                        span.end().line(),
                        span.end().column(),
                    )
                })
                .collect::<Vec<_>>()
        };
        let first = project(&files);
        let expected = [
            "struct|[]|Foo|src/root.rs|2:8-2:11",
            "enum|[alpha]|Ordinary|src/alpha.rs|2:6-2:14",
            "struct|[alpha::deep]|Deep|src/alpha/deep/mod.rs|2:8-2:12",
            "enum|[inline]|Fault|src/root.rs|6:6-6:11",
            "struct|[inline]|foo|src/root.rs|8:8-8:13",
        ];
        assert_eq!(first, expected);
        assert_eq!(first, project(&files.into_iter().rev().collect::<Vec<_>>()));
    }

    #[test]
    fn placement_accepts_exact_markers_on_reachable_module_scope_structs_and_enums() {
        let files = [
            (
                "root.rs",
                "#[boxology::contract]\nstruct Root;\nmod outline;\nmod inline { #[r#boxology::r#contract] enum Inline { A } }\n",
            ),
            (
                "outline.rs",
                "#[::boxology::contract]\nstruct Outline;\n#[boxology::contract(error)]\nenum Fault { A }\n",
            ),
        ];
        for reversed in [false, true] {
            let parsed =
                ParsedRustInputs::parse(&request_in_order("root.rs", &files, reversed)).unwrap();
            assert_eq!(
                parsed
                    .discover_contract_declarations()
                    .unwrap()
                    .iter()
                    .map(|declaration| (declaration.lifted_name(), declaration.role()))
                    .collect::<Vec<_>>(),
                [
                    ("Root", ContractDeclarationRole::Value),
                    ("Inline", ContractDeclarationRole::Value),
                    ("Outline", ContractDeclarationRole::Value),
                    ("Fault", ContractDeclarationRole::Error),
                ]
            );
        }
    }

    #[test]
    fn placement_visitor_covers_structured_attribute_sites_with_exact_leaf_spans() {
        let source = concat!(
            "#[::boxology::contract]\nconst ITEM_CONST: u8 = 0;\n",
            "#[::boxology::contract]\nextern crate core as item_extern;\n",
            "#[::boxology::contract]\nfn item_function() {}\n",
            "#[::boxology::contract]\nextern \"C\" {\n",
            "#[::boxology::contract]\nfn foreign_function();\n",
            "#[::boxology::contract]\nstatic FOREIGN_STATIC: u8;\n",
            "#[::boxology::contract]\ntype ForeignType;\n",
            "#[::boxology::contract]\nforeign_macro!();\n}\n",
            "#[::boxology::contract]\nmacro_item!();\n",
            "#[::boxology::contract]\nmod item_module {}\n",
            "#[::boxology::contract]\nstatic ITEM_STATIC: u8 = 0;\n",
            "#[::boxology::contract]\ntrait ItemTrait {\n",
            "#[::boxology::contract]\nconst TRAIT_CONST: u8;\n",
            "#[::boxology::contract]\nfn trait_method();\n",
            "#[::boxology::contract]\ntype TraitType;\n",
            "#[::boxology::contract]\ntrait_macro!();\n}\n",
            "#[::boxology::contract]\ntrait ItemTraitAlias = Send;\n",
            "#[::boxology::contract]\ntype ItemType = u8;\n",
            "type NestedType = fn(\n#[::boxology::contract]\nu8\n);\n",
            "#[::boxology::contract]\nunion ItemUnion {\n",
            "#[::boxology::contract]\nunion_field: u8,\n}\n",
            "#[::boxology::contract]\nuse core::fmt as item_use;\n",
            "struct Host;\n#[::boxology::contract]\nimpl Host {\n",
            "#[::boxology::contract]\nconst IMPL_CONST: u8 = 0;\n",
            "#[::boxology::contract]\nfn impl_method() {\n#[::boxology::contract]\nstruct MethodLocal;\n}\n",
            "#[::boxology::contract]\ntype ImplType = u8;\n",
            "#[::boxology::contract]\nimpl_macro!();\n}\n",
            "struct PlainNamed {\n#[::boxology::contract]\nnamed: u8,\n}\n",
            "struct PlainTuple(\n#[::boxology::contract]\nu8);\n",
            "enum PlainEnum {\n#[::boxology::contract]\nUnit,\nNamed {\n",
            "#[::boxology::contract]\nfield: u8 },\nTuple(\n#[::boxology::contract]\nu8),\n}\n",
            "#[boxology::contract]\nstruct ContractNamed {\n#[::boxology::contract]\nfield: u8,\n}\n",
            "#[boxology::contract]\nstruct ContractTuple(\n#[::boxology::contract]\nu8);\n",
            "#[boxology::contract]\nenum ContractEnum {\n#[::boxology::contract]\nUnit,\n",
            "Named {\n#[::boxology::contract]\nfield: u8 },\n",
            "Tuple(\n#[::boxology::contract]\nu8),\n}\n",
            "#[::boxology::contract]\nfn contexts<\n#[::boxology::contract]\nT\n>(\n",
            "#[::boxology::contract]\nargument: T\n) {\n",
            "#[::boxology::contract]\nstruct Local;\n",
            "struct Pattern { field: u8 }\n#[::boxology::contract]\nlet Pattern {\n",
            "#[::boxology::contract]\nfield,\n} = \n#[::boxology::contract]\nPattern { field: 0 };\n",
            "struct Record { value: u8 }\nlet _ = Record {\n",
            "#[::boxology::contract]\nvalue: 0 };\nmatch 0 {\n",
            "#[::boxology::contract]\narm => arm,\n};\n}\n",
        );
        let diagnostics = discovery_errors(&request("root.rs", &[("root.rs", source)]));
        let expected = source
            .lines()
            .enumerate()
            .filter(|(_, line)| line.contains("#[::boxology::contract]"))
            .map(|(line, _)| span((line + 1, 15), (line + 1, 23)))
            .collect::<Vec<_>>();
        assert_eq!(diagnostics.as_slice().len(), expected.len());
        for (diagnostic, expected_span) in diagnostics.as_slice().iter().zip(expected) {
            assert_eq!(diagnostic.code(), "BXG0029");
            assert_eq!(diagnostic.path().as_str(), "root.rs");
            assert_eq!(diagnostic.span(), expected_span);
            assert_eq!(
                diagnostic.offending_construct(),
                "misplaced contract declaration annotation"
            );
            assert_eq!(diagnostic.rule(), CONTRACT_PLACEMENT_RULE);
            assert_eq!(diagnostic.rule_source(), CONTRACT_PLACEMENT_RULE_SOURCE);
        }
    }

    #[test]
    fn misplaced_forms_are_payload_safe_and_never_use_contract_role_validation() {
        let source = concat!(
            "#[boxology::contract]\nfn PrivatePath() {}\n",
            "#[boxology::contract(PrivateList)]\nfn PrivateListOwner() {}\n",
            "#[boxology::contract = \"PrivateNameValue\"]\nfn PrivateValueOwner() {}\n",
        );
        let diagnostics = discovery_errors(&request("root.rs", &[("root.rs", source)]));
        assert_eq!(diagnostics.as_slice().len(), 3);
        for (diagnostic, line) in diagnostics.as_slice().iter().zip([1, 3, 5]) {
            assert_eq!(diagnostic.code(), "BXG0029");
            assert_eq!(diagnostic.span(), span((line, 13), (line, 21)));
        }
        let rendered = format!("{diagnostics}\n{diagnostics:?}");
        assert!(!rendered.contains("BXG0024"));
        for sentinel in [
            "PrivatePath",
            "PrivateList",
            "PrivateListOwner",
            "PrivateNameValue",
            "PrivateValueOwner",
        ] {
            assert!(!rendered.contains(sentinel));
        }
    }

    #[test]
    fn placement_findings_aggregate_by_path_and_ignore_request_order() {
        let files = [
            (
                "root.rs",
                "mod z;\nmod a;\n#[::boxology::contract]\nfn root_marker() {}\n",
            ),
            (
                "a.rs",
                "struct A {\n#[::boxology::contract]\nfield: u8,\n}\n",
            ),
            ("z.rs", "#[::boxology::contract]\nfn z_marker() {}\n"),
        ];
        let first = discovery_errors(&request_in_order("root.rs", &files, false));
        assert_eq!(
            first,
            discovery_errors(&request_in_order("root.rs", &files, true))
        );
        assert_eq!(
            first
                .as_slice()
                .iter()
                .map(|diagnostic| (diagnostic.path().as_str(), diagnostic.span()))
                .collect::<Vec<_>>(),
            [
                ("a.rs", span((2, 15), (2, 23))),
                ("root.rs", span((3, 15), (3, 23))),
                ("z.rs", span((1, 15), (1, 23))),
            ]
        );
    }

    #[test]
    fn every_earlier_phase_suppresses_contract_placement_validation() {
        let syntax = parse_errors(&request(
            "root.rs",
            &[
                ("root.rs", "#[::boxology::contract] fn misplaced() {}"),
                ("broken.rs", "fn broken() { @ }"),
            ],
        ));
        assert!(
            syntax
                .as_slice()
                .iter()
                .all(|diagnostic| diagnostic.code() == "BXG0014")
        );
        let suppressed = |code, source| {
            let diagnostics = discovery_errors(&request("root.rs", &[("root.rs", source)]));
            assert!(
                diagnostics
                    .as_slice()
                    .iter()
                    .all(|diagnostic| diagnostic.code() == code)
            );
            assert!(!diagnostics.to_string().contains("BXG0029"));
        };
        suppressed(
            "BXG0017",
            "mod missing; #[::boxology::contract] fn misplaced() {}",
        );
        let unreachable = discovery_errors(&request(
            "root.rs",
            &[
                ("root.rs", "#[::boxology::contract] fn misplaced() {}"),
                ("dead.rs", "#[boxology::contract] fn dead() {}"),
            ],
        ));
        assert!(
            unreachable
                .as_slice()
                .iter()
                .all(|diagnostic| diagnostic.code() == "BXG0019")
        );
        assert!(!unreachable.to_string().contains("BXG0029"));
        suppressed(
            "BXG0020",
            "#[cfg(Private)] #[boxology::contract] struct S; #[::boxology::contract] fn misplaced() {}",
        );
        suppressed(
            "BXG0021",
            "#[boxology::contract] struct S; #[boxology::contract] enum S { A } #[::boxology::contract] fn misplaced() {}",
        );
        suppressed(
            "BXG0024",
            "#[boxology::contract(Private)] struct S; #[::boxology::contract] fn misplaced() {}",
        );
        suppressed(
            "BXG0028",
            "#[boxology::contract] enum E { Same, Same } #[::boxology::contract] fn misplaced() {}",
        );
    }

    #[test]
    fn capability_placement_rejects_structured_sites_exactly_without_payload_leaks() {
        let source = "#[::boxology::capability]\nfn PrivatePath() {}\ntrait T {\n#[::boxology::capability(PrivateList)]\nfn PrivateTrait();\n}\nstruct Host; impl T for Host {\n#[::boxology::capability = \"PrivateValue\"]\nfn PrivateTraitImpl(&self) {}\n}\nimpl Host {\n#[::boxology::capability]\nconst PRIVATE_CONST: u8 = 0;\n}\nstruct Nested {\n#[::boxology::capability]\nfield: u8,\n}\nfn nested() {\n#[::boxology::capability]\nlet local = 0;\n}\nmacro_rules! hidden { () => { #[boxology::capability] fn token() {} } }\n#[holder = { #[boxology::capability] 1 }] fn payload() {}";
        let diagnostics = capability_errors(&request("root.rs", &[("root.rs", source)]));
        let lines = [1, 4, 8, 12, 16, 20];
        for (diagnostic, line) in diagnostics.as_slice().iter().zip(lines) {
            assert_eq!(diagnostic.code(), "BXG0030");
            assert_eq!(diagnostic.span(), span((line, 15), (line, 25)));
        }
        let rendered = format!("{diagnostics}\n{diagnostics:?}");
        for sentinel in ["PrivatePath", "PrivateList", "PrivateValue", "local"] {
            assert!(!rendered.contains(sentinel));
        }
    }

    #[test]
    fn all_predecessor_phases_suppress_capability_placement() {
        let suppressed = |code, source| {
            let diagnostics = capability_errors(&request("root.rs", &[("root.rs", source)]));
            assert!(
                diagnostics
                    .as_slice()
                    .iter()
                    .all(|error| error.code() == code)
            );
            diagnostics
        };
        for (code, source) in [
            (
                "BXG0017",
                "mod missing; #[boxology::capability] fn misplaced() {}",
            ),
            (
                "BXG0021",
                "#[boxology::contract] struct S; #[boxology::contract] enum S { A } #[boxology::capability] fn misplaced() {}",
            ),
            (
                "BXG0029",
                "#[boxology::contract] fn bad() {} #[boxology::capability] fn misplaced() {}",
            ),
        ] {
            suppressed(code, source);
        }
        let module_cfg = suppressed(
            "BXG0020",
            "fn block() {\n#[cfg(Ancestor)]\nmod local { struct H; impl H { #[boxology::capability] fn method() {} } }\n}",
        );
        assert_eq!(module_cfg.as_slice().len(), 1);
        assert_eq!(module_cfg.as_slice()[0].span(), span((2, 3), (2, 6)));
        let unreachable = capability_errors(&request(
            "root.rs",
            &[
                ("root.rs", "#[boxology::capability] fn misplaced() {}"),
                (
                    "dead.rs",
                    "trait T { #[boxology::capability(Private)] fn method(); } struct S { #[boxology::capability] field: u8 } fn f() { #[boxology::capability] let local = 0; struct H; impl H { #[boxology::capability] fn nested() {} } } macro_rules! hidden { () => { #[boxology::capability] fn token() {} } }",
                ),
            ],
        ));
        assert_eq!(unreachable.as_slice()[0].code(), "BXG0019");
        assert_eq!(unreachable.as_slice().len(), 4);
        assert!(!format!("{unreachable:?}").contains("Private"));
    }

    #[test]
    fn capability_call_shapes_accept_only_the_structural_frame_without_reading_types() {
        let source = "struct Host; impl Host { #[boxology::capability] pub async fn public(&self, request: PrivateGeneric<'_>, context: &'static mut PrivateContext) -> impl PrivateOutput { loop {} } #[boxology::capability(PrivateMetadata)] async fn private(&self, _: [Private; 7], _: fn(Private) -> Private) -> PrivateReturn { loop {} } }";
        let parsed = ParsedRustInputs::parse(&request("root.rs", &[("root.rs", source)])).unwrap();
        let validated = parsed.validate_capability_call_shapes().unwrap();
        assert_eq!(
            validated
                .iter()
                .map(|declaration| declaration.method().sig.ident.to_string())
                .collect::<Vec<_>>(),
            ["public", "private"]
        );
    }

    #[test]
    fn capability_call_shape_diagnostics_are_complete_exact_and_payload_safe() {
        let source = concat!(
            "struct Host; impl Host {\n",
            "#[boxology::capability] fn Sync(&self, a: PrivateA, b: PrivateB) -> PrivateR {}\n",
            "#[boxology::capability] async fn MissingReceiver(a: PrivateA, b: PrivateB) -> PrivateR {}\n",
            "#[boxology::capability] async fn ByValue(self, a: PrivateA, b: PrivateB) -> PrivateR {}\n",
            "#[boxology::capability] async fn Mutable(&mut self, a: PrivateA, b: PrivateB) -> PrivateR {}\n",
            "#[boxology::capability] async fn Typed(self: &Self, a: PrivateA, b: PrivateB) -> PrivateR {}\n",
            "#[boxology::capability] async fn Zero(&self) -> PrivateR {}\n",
            "#[boxology::capability] async fn One(&self, a: PrivateA) -> PrivateR {}\n",
            "#[boxology::capability] async fn Three(&self, a: PrivateA, b: PrivateB, c: PrivateC) -> PrivateR {}\n",
            "#[boxology::capability] async fn MissingReturn(&self, a: PrivateA, b: PrivateB) {}\n",
            "#[boxology::capability] async unsafe extern \"C\" fn Variadic(&self, a: PrivateA, b: PrivateB, ...) -> PrivateR {}\n",
            "#[boxology::capability(PrivateAttribute)] fn EverythingWrong(self) {}\n",
            "}\n",
        );
        let request = request("root.rs", &[("root.rs", source)]);
        assert_eq!(
            ParsedRustInputs::parse(&request)
                .unwrap()
                .discover_capability_declarations()
                .unwrap()
                .len(),
            11
        );
        let diagnostics = capability_shape_errors(&request);
        assert_eq!(diagnostics.as_slice().len(), 11);
        let identifiers = [
            "Sync",
            "MissingReceiver",
            "ByValue",
            "Mutable",
            "Typed",
            "Zero",
            "One",
            "Three",
            "MissingReturn",
            "Variadic",
            "EverythingWrong",
        ];
        for (diagnostic, (line, identifier)) in
            diagnostics.as_slice().iter().zip((2..=12).zip(identifiers))
        {
            let column = source
                .lines()
                .nth(line - 1)
                .unwrap()
                .find(identifier)
                .unwrap()
                + 1;
            assert_eq!(diagnostic.code(), "BXG0031");
            assert_eq!(
                diagnostic.span(),
                span((line, column), (line, column + identifier.len()))
            );
            assert_eq!(
                diagnostic.offending_construct(),
                "invalid structural capability signature"
            );
            assert_eq!(diagnostic.rule(), CAPABILITY_CALL_SHAPE_RULE);
            assert_eq!(diagnostic.rule_source(), CAPABILITY_CALL_SHAPE_RULE_SOURCE);
        }
        let rendered = format!("{diagnostics}\n{diagnostics:?}");
        for private in [
            "PrivateA",
            "PrivateR",
            "EverythingWrong",
            "PrivateAttribute",
        ] {
            assert!(!rendered.contains(private));
        }
    }

    #[test]
    fn capability_call_shape_results_are_deterministic_across_modules_and_input_order() {
        let files = [
            (
                "root.rs",
                "mod z; mod a; struct R; impl R { #[boxology::capability] async fn root(&self, a: A, b: B) -> R { loop {} } }",
            ),
            (
                "a.rs",
                "struct A; impl A { #[boxology::capability] fn invalid_a(&self) {} }",
            ),
            (
                "z.rs",
                "struct Z; impl Z { #[boxology::capability] fn invalid_z(&self, a: A, b: B) {} }",
            ),
        ];
        let evaluate = |reversed| {
            let request = request_in_order("root.rs", &files, reversed);
            let parsed = ParsedRustInputs::parse(&request).unwrap();
            parsed
                .validate_capability_call_shapes()
                .map(|declarations| {
                    declarations
                        .iter()
                        .map(|item| {
                            (
                                item.source().as_str().to_owned(),
                                item.module_path().to_vec(),
                                item.identifier_span(),
                            )
                        })
                        .collect::<Vec<_>>()
                })
        };
        let first = evaluate(false).unwrap_err();
        let second = evaluate(true).unwrap_err();
        assert_eq!(first, second);
        assert_eq!(
            first
                .as_slice()
                .iter()
                .map(|diagnostic| (diagnostic.path().as_str(), diagnostic.span().start().line()))
                .collect::<Vec<_>>(),
            [("a.rs", 1), ("z.rs", 1)]
        );
    }

    #[test]
    fn every_predecessor_phase_suppresses_capability_call_shape_validation() {
        let invalid = "struct H; impl H { #[boxology::capability] fn invalid() {} }";
        let syntax = parse_errors(&request(
            "root.rs",
            &[("root.rs", invalid), ("broken.rs", "fn broken() { @ }")],
        ));
        assert!(
            syntax
                .as_slice()
                .iter()
                .all(|diagnostic| diagnostic.code() == "BXG0014")
        );
        assert!(!syntax.to_string().contains("BXG0031"));
        let cases: &[(&str, &[(&str, &str)])] = &[
            (
                "BXG0016",
                &[(
                    "root.rs",
                    "#[path=\"x\"] mod x; struct H; impl H { #[boxology::capability] fn invalid() {} }",
                )],
            ),
            (
                "BXG0017",
                &[(
                    "root.rs",
                    "mod missing; struct H; impl H { #[boxology::capability] fn invalid() {} }",
                )],
            ),
            (
                "BXG0018",
                &[
                    ("root.rs", "mod both;"),
                    ("both.rs", ""),
                    ("both/mod.rs", ""),
                ],
            ),
            (
                "BXG0019",
                &[
                    ("root.rs", invalid),
                    ("dead.rs", "#[boxology::contract] struct Dead;"),
                ],
            ),
            (
                "BXG0020",
                &[("root.rs", "#[cfg(x)] #[boxology::contract] struct S;")],
            ),
            (
                "BXG0021",
                &[(
                    "root.rs",
                    "#[boxology::contract] struct S; #[boxology::contract] enum S { A }",
                )],
            ),
            (
                "BXG0022",
                &[("root.rs", "#[Private] #[boxology::contract] struct S;")],
            ),
            (
                "BXG0023",
                &[("root.rs", "#[derive(Copy)] #[boxology::contract] struct S;")],
            ),
            (
                "BXG0024",
                &[("root.rs", "#[boxology::contract(Private)] struct S;")],
            ),
            (
                "BXG0025",
                &[(
                    "root.rs",
                    "#[deprecated(Private)] #[boxology::contract] struct S;",
                )],
            ),
            (
                "BXG0026",
                &[("root.rs", "#[doc] #[boxology::contract] struct S;")],
            ),
            (
                "BXG0027",
                &[("root.rs", "#[boxology::contract] struct S { a: u8, a: u8 }")],
            ),
            (
                "BXG0028",
                &[("root.rs", "#[boxology::contract] enum S { A, A }")],
            ),
            (
                "BXG0029",
                &[("root.rs", "#[boxology::contract] fn bad() {}")],
            ),
            (
                "BXG0030",
                &[("root.rs", "#[boxology::capability] fn bad() {}")],
            ),
        ];
        for (code, files) in cases {
            let diagnostics = capability_shape_errors(&request("root.rs", files));
            assert!(
                diagnostics
                    .as_slice()
                    .iter()
                    .all(|diagnostic| diagnostic.code() == *code)
            );
            assert!(!diagnostics.to_string().contains("BXG0031"));
        }
    }

    #[test]
    fn capability_metadata_projects_defaults_overrides_and_deterministic_order() {
        let files = [
            (
                "root.rs",
                "mod child; struct H; impl H { #[boxology::capability] async fn r#match(&self, a: A, b: B) -> R { loop {} } #[boxology::capability(name = \"Bad-Override\", exposure = \"code-only\", idempotency = \"none\",)] async fn ordered(&self, a: A, b: B) -> R { loop {} } } mod inline { struct I; impl I { #[boxology::capability(idempotency = \"inherent\", exposure = \"internal\", name = \"inside\")] async fn inner(&self, a: A, b: B) -> R { loop {} } } }",
            ),
            (
                "child.rs",
                "struct C; impl C { #[boxology::capability(name = \"greet\", exposure = \"external\")] async fn hello(&self, a: A, b: B) -> R { loop {} } }",
            ),
        ];
        let evaluate = |reversed| {
            let request = request_in_order("root.rs", &files, reversed);
            let parsed = ParsedRustInputs::parse(&request).unwrap();
            parsed
                .validate_capability_marker_metadata()
                .unwrap()
                .iter()
                .map(|item| {
                    format!(
                        "{:?}|{:?}|{:?}|{:?}",
                        item.module_path(),
                        item.marker_metadata().name_override(),
                        item.marker_metadata().max_exposure(),
                        item.marker_metadata().idempotency(),
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        let expected = "[]|None|CodeOnly|None\n[]|Some(\"Bad-Override\")|CodeOnly|None\n[\"child\"]|Some(\"greet\")|External|None\n[\"inline\"]|Some(\"inside\")|Internal|Inherent";
        assert_eq!(evaluate(false), expected);
        assert_eq!(evaluate(true), expected);
    }

    #[test]
    fn capability_metadata_syntax_diagnostics_are_exact_and_payload_safe() {
        let cases = [
            ("#[boxology::capability()]", "capability"),
            ("#[boxology::capability = \"x\"]", "capability"),
            ("#[boxology::capability(\"x\")]", "capability"),
            (
                "#[boxology::capability(name = \"x\" name = \"y\")]",
                "capability",
            ),
            ("#[boxology::capability(name = 7)]", "capability"),
            (
                "#[boxology::capability(DO_NOT_LEAK = \"x\")]",
                "DO_NOT_LEAK",
            ),
            (
                "#[boxology::capability(name = \"a\", name = \"b\")]",
                "name",
            ),
            (
                "#[boxology::capability] #[boxology::capability]",
                "capability",
            ),
            (
                "#[boxology::capability(exposure = \"private\")]",
                "\"private\"",
            ),
            (
                "#[boxology::capability(idempotency = \"sometimes\")]",
                "\"sometimes\"",
            ),
        ];
        for (attributes, expected_span) in cases {
            assert_metadata_error(attributes, "BXG0032", expected_span);
        }
    }

    #[test]
    fn capability_metadata_debug_and_equality_ignore_private_source_spans() {
        let left_request = request(
            "root.rs",
            &[(
                "root.rs",
                "struct H; impl H { #[boxology::capability(name = \"same\", exposure = \"internal\")] async fn a(&self, x: A, y: B) -> R { loop {} } }",
            )],
        );
        let right_request = request(
            "root.rs",
            &[(
                "root.rs",
                "struct H; impl H {              #[boxology::capability(name = \"same\", exposure = \"internal\")] async fn a(&self, x: A, y: B) -> R { loop {} } }",
            )],
        );
        let left_parsed = ParsedRustInputs::parse(&left_request).unwrap();
        let right_parsed = ParsedRustInputs::parse(&right_request).unwrap();
        let left_declarations = left_parsed.validate_capability_marker_metadata().unwrap();
        let right_declarations = right_parsed.validate_capability_marker_metadata().unwrap();
        let left = left_declarations[0].marker_metadata();
        let right = right_declarations[0].marker_metadata();
        assert_ne!(left.name_override_span, right.name_override_span);
        assert_eq!(left, right);
        let debug = format!("{left:?}");
        assert_eq!(debug, format!("{right:?}"));
        assert!(!debug.contains("span") && !debug.contains("LineColumn"));
    }

    #[test]
    fn unsupported_metadata_is_staged_exactly() {
        for (entry, expected_span) in [
            ("idempotency = \"keyed\"", "\"keyed\""),
            ("auth = \"DO_NOT_LEAK\"", "auth"),
            ("default = \"DO_NOT_LEAK\"", "default"),
            ("min = \"DO_NOT_LEAK\"", "min"),
            ("max = \"DO_NOT_LEAK\"", "max"),
            ("validation = \"DO_NOT_LEAK\"", "validation"),
        ] {
            assert_metadata_error(
                &format!("#[boxology::capability({entry})]"),
                "BXG0033",
                expected_span,
            );
        }
    }

    #[test]
    fn capability_metadata_is_suppressed_by_predecessor_and_shape_failures() {
        for (source, code) in [
            (
                "#[boxology::capability(exposure = \"bad\")] fn free() {}",
                "BXG0030",
            ),
            (
                "struct H; impl H { #[boxology::capability(exposure = \"bad\")] fn sync(&self) {} }",
                "BXG0031",
            ),
        ] {
            let diagnostics =
                capability_metadata_errors(&request("root.rs", &[("root.rs", source)]));
            assert!(
                diagnostics
                    .as_slice()
                    .iter()
                    .all(|item| item.code() == code)
            );
            assert!(!diagnostics.to_string().contains("BXG0032"));
        }
    }

    #[test]
    fn capability_identities_project_effective_qualified_names_and_retain_metadata() {
        let files = [
            (
                "root.rs",
                "mod child; struct A; impl A { #[boxology::capability] async fn greet(&self, a: A, b: B) -> R { loop {} } #[boxology::capability(name = \"rescued\", exposure = \"external\", idempotency = \"inherent\")] async fn BadName(&self, a: A, b: B) -> R { loop {} } #[boxology::capability] async fn r#type(&self, a: A, b: B) -> R { loop {} } #[boxology::capability] async fn a0__(&self, a: A, b: B) -> R { loop {} } }",
            ),
            (
                "child.rs",
                "struct B; impl B { #[boxology::capability(name = \"child_name\")] async fn ignored(&self, a: A, b: B) -> R { loop {} } }",
            ),
        ];
        let evaluate = |reversed| {
            ParsedRustInputs::parse(&request_in_order("root.rs", &files, reversed))
                .unwrap()
                .validate_capability_identities()
                .unwrap()
                .iter()
                .map(|item| {
                    format!(
                        "{}|{}|{:?}|{:?}|{:?}",
                        item.id(),
                        item.id().name(),
                        item.marker_metadata().name_override(),
                        item.marker_metadata().max_exposure(),
                        item.marker_metadata().idempotency()
                    )
                })
                .collect::<Vec<_>>()
        };
        let expected = [
            "demo.greet|greet|None|CodeOnly|None",
            "demo.rescued|rescued|Some(\"rescued\")|External|Inherent",
            "demo.type|type|None|CodeOnly|None",
            "demo.a0__|a0__|None|CodeOnly|None",
            "demo.child_name|child_name|Some(\"child_name\")|CodeOnly|None",
        ];
        assert_eq!(evaluate(false), expected);
        assert_eq!(evaluate(true), expected);
    }

    #[test]
    fn invalid_effective_names_are_complete_exact_ordered_and_payload_safe() {
        let source = "struct H; impl H { #[boxology::capability] async fn BadName(&self, a: A, b: B) -> R { loop {} } #[boxology::capability] async fn _hidden(&self, a: A, b: B) -> R { loop {} } #[boxology::capability(name = \"\")] async fn a(&self, a: A, b: B) -> R { loop {} } #[boxology::capability(name = \"Upper\")] async fn b(&self, a: A, b: B) -> R { loop {} } #[boxology::capability(name = \"bad-name\")] async fn c(&self, a: A, b: B) -> R { loop {} } #[boxology::capability(name = \"DO_NOT_LEAK_\\u{e9}\")] async fn d(&self, a: A, b: B) -> R { loop {} } }";
        let diagnostics = capability_identity_errors(&request("root.rs", &[("root.rs", source)]));
        assert_eq!(diagnostics.as_slice().len(), 6);
        for (diagnostic, expected) in diagnostics.as_slice().iter().zip([
            "BadName",
            "_hidden",
            "\"\"",
            "\"Upper\"",
            "\"bad-name\"",
            "\"DO_NOT_LEAK_\\u{e9}\"",
        ]) {
            assert_eq!(diagnostic.code(), "BXG0034");
            let span = diagnostic.span();
            assert_eq!(
                &source[span.start().column() - 1..span.end().column() - 1],
                expected
            );
            assert_eq!(diagnostic.rule_source(), CAPABILITY_IDENTITY_RULE_SOURCE);
        }
        let rendered = format!("{diagnostics}\n{diagnostics:?}");
        assert!(!rendered.contains("DO_NOT_LEAK") && !rendered.contains("InvalidCapabilityName"));
    }

    #[test]
    fn capability_identity_collisions_report_every_later_declaration_deterministically() {
        let files = [
            (
                "root.rs",
                "mod child; struct A; impl A { #[boxology::capability] async fn do_not_leak(&self, a: A, b: B) -> R { loop {} } } struct B; impl B { #[boxology::capability(name = \"do_not_leak\")] async fn second(&self, a: A, b: B) -> R { loop {} } #[boxology::capability(name = \"do_not_leak\")] async fn third(&self, a: A, b: B) -> R { loop {} } }",
            ),
            (
                "child.rs",
                "struct C; impl C { #[boxology::capability] async fn do_not_leak(&self, a: A, b: B) -> R { loop {} } }",
            ),
        ];
        let evaluate =
            |reversed| capability_identity_errors(&request_in_order("root.rs", &files, reversed));
        let diagnostics = evaluate(false);
        assert_eq!(diagnostics, evaluate(true));
        assert_eq!(diagnostics.as_slice().len(), 3);
        for (diagnostic, (path, source, expected)) in diagnostics.as_slice().iter().zip([
            ("child.rs", files[1].1, "do_not_leak"),
            ("root.rs", files[0].1, "\"do_not_leak\""),
            ("root.rs", files[0].1, "\"do_not_leak\""),
        ]) {
            assert_eq!(
                (diagnostic.code(), diagnostic.path().as_str()),
                ("BXG0035", path)
            );
            let span = diagnostic.span();
            assert_eq!(
                &source[span.start().column() - 1..span.end().column() - 1],
                expected
            );
        }
        assert!(!format!("{diagnostics}\n{diagnostics:?}").contains("do_not_leak"));
    }

    #[test]
    fn capability_identity_predecessors_and_invalid_names_suppress_later_phases() {
        for (source, code) in [
            ("#[boxology::capability] fn free() {}", "BXG0030"),
            (
                "struct H; impl H { #[boxology::capability] fn sync(&self) {} }",
                "BXG0031",
            ),
            (
                "struct H; impl H { #[boxology::capability(exposure = \"bad\")] async fn a(&self, x: A, y: B) -> R { loop {} } }",
                "BXG0032",
            ),
            (
                "struct H; impl H { #[boxology::capability(idempotency = \"keyed\")] async fn a(&self, x: A, y: B) -> R { loop {} } }",
                "BXG0033",
            ),
        ] {
            let diagnostics =
                capability_identity_errors(&request("root.rs", &[("root.rs", source)]));
            assert!(
                diagnostics
                    .as_slice()
                    .iter()
                    .all(|item| item.code() == code)
            );
        }
        let source = "struct H; impl H { #[boxology::capability] async fn BadName(&self, x: A, y: B) -> R { loop {} } #[boxology::capability] async fn dup(&self, x: A, y: B) -> R { loop {} } #[boxology::capability(name = \"dup\")] async fn other(&self, x: A, y: B) -> R { loop {} } }";
        let diagnostics = capability_identity_errors(&request("root.rs", &[("root.rs", source)]));
        assert_eq!(diagnostics.as_slice().len(), 1);
        assert_eq!(diagnostics.as_slice()[0].code(), "BXG0034");
        assert!(!diagnostics.to_string().contains("BXG0035"));
    }

    #[test]
    fn raw_struct_and_enum_collisions_are_complete_repeatable_exact_and_payload_safe() {
        let files = [
            (
                "root.rs",
                "#[boxology::contract(root_payload)]\nstruct Foo { winner_field: u8 }\nmod a;\nmod z;\n",
            ),
            (
                "a.rs",
                "#[boxology::contract(loser_payload)]\nstruct r#Foo { loser_field: u8 }\n",
            ),
            (
                "z.rs",
                "#[boxology::contract(error)]\nenum Foo { SecretVariant }\n",
            ),
        ];
        let first = discovery_errors(&request("root.rs", &files));
        let reversed = files.into_iter().rev().collect::<Vec<_>>();
        assert_eq!(first, discovery_errors(&request("root.rs", &reversed)));
        assert_eq!(first.as_slice().len(), 2);
        for (diagnostic, (path, expected_span)) in first.as_slice().iter().zip([
            ("a.rs", span((2, 8), (2, 13))),
            ("z.rs", span((2, 6), (2, 9))),
        ]) {
            assert_eq!(
                (diagnostic.code(), diagnostic.path().as_str()),
                ("BXG0021", path)
            );
            assert_eq!(diagnostic.span(), expected_span);
            assert_eq!(
                diagnostic.offending_construct(),
                "colliding lifted contract type name"
            );
            assert_eq!(diagnostic.rule(), COLLISION_RULE);
            assert_eq!(diagnostic.rule_source(), COLLISION_RULE_SOURCE);
        }
        let rendered = first.to_string();
        for sentinel in ["Foo", "payload", "field", "SecretVariant", "root.rs"] {
            assert!(!rendered.contains(sentinel));
        }
    }

    #[test]
    fn contract_attribute_allowlist_accepts_all_supported_sites_and_forms() {
        let source = concat!(
            "/// declaration docs\n#[doc = \"more docs\"]\n#[deprecated]\n",
            "#[derive(Debug, r#Clone)]\n#[derive(PartialEq)]\n#[boxology::contract]\n",
            "struct Named { #[doc = \"field\"] #[derive(Clone)] named: u8 }\n",
            "#[::boxology::contract]\n",
            "struct Tuple(#[::boxology::field(anything)] #[deprecated(note = \"later\")] u8);\n",
            "#[derive()]\n#[boxology::contract(error)]\nenum Event {\n",
            "#[doc = \"variant\"] Unit,\n",
            "Tuple(#[derive(Debug, PartialEq)] u8),\n",
            "Named { #[boxology::field] value: u8 },\n}\n",
        );
        let parsed = ParsedRustInputs::parse(&request("root.rs", &[("root.rs", source)])).unwrap();
        let declarations = parsed.discover_contract_declarations().unwrap();
        assert_eq!(
            declarations
                .iter()
                .map(ContractDeclaration::lifted_name)
                .collect::<Vec<_>>(),
            ["Named", "Tuple", "Event"]
        );
    }

    #[test]
    fn contract_roles_accept_exact_forms_and_are_input_order_invariant() {
        let files = [
            (
                "root.rs",
                "mod a; mod z;\n#[r#boxology::r#contract]\nstruct Root;\n",
            ),
            ("a.rs", "#[::boxology::contract]\nenum Ordinary { A }\n"),
            (
                "z.rs",
                "#[::r#boxology::r#contract(r#error,)]\nenum Failure { A }\n",
            ),
        ];
        let project = |reversed| {
            ParsedRustInputs::parse(&request_in_order("root.rs", &files, reversed))
                .unwrap()
                .discover_contract_declarations()
                .unwrap()
                .into_iter()
                .map(|declaration| {
                    (
                        declaration.lifted_name().to_owned(),
                        declaration.role(),
                        matches!(declaration.syntax(), ContractDeclarationSyntax::Struct(_)),
                    )
                })
                .collect::<Vec<_>>()
        };
        let canonical = project(false);
        assert_eq!(canonical, project(true));
        assert_eq!(
            canonical,
            [
                ("Root".into(), ContractDeclarationRole::Value, true),
                ("Ordinary".into(), ContractDeclarationRole::Value, false),
                ("Failure".into(), ContractDeclarationRole::Error, false),
            ]
        );
    }

    #[test]
    fn invalid_contract_roles_are_exact_complete_repeatable_and_payload_safe() {
        let files = [
            ("root.rs", "mod z; mod a;\n"),
            (
                "a.rs",
                concat!(
                    "#[boxology::contract(error)] struct BadStruct;\n",
                    "#[boxology::contract()] enum Empty { A }\n",
                    "#[boxology::contract(error, PrivateMany)] enum Many { A }\n",
                    "#[boxology::contract(PrivatePath::error)] enum Qualified { A }\n",
                ),
            ),
            (
                "z.rs",
                concat!(
                    "#[boxology::contract(error(PrivateNested))] enum Nested { A }\n",
                    "#[boxology::contract(error::<PrivateType>)] enum Parameterized { A }\n",
                    "#[boxology::contract = \"PrivateLiteral\"] enum Assigned { A }\n",
                    "#[boxology::contract(PrivateMarker)]\n",
                    "#[boxology::contract]\n",
                    "enum Duplicate { PrivateVariant }\n",
                ),
            ),
        ];
        let first = discovery_errors(&request_in_order("root.rs", &files, false));
        assert_eq!(
            first,
            discovery_errors(&request_in_order("root.rs", &files, false))
        );
        assert_eq!(
            first,
            discovery_errors(&request_in_order("root.rs", &files, true))
        );
        let expected = [
            ("a.rs", 1),
            ("a.rs", 2),
            ("a.rs", 3),
            ("a.rs", 4),
            ("z.rs", 1),
            ("z.rs", 2),
            ("z.rs", 3),
            ("z.rs", 4),
            ("z.rs", 5),
        ];
        assert_eq!(first.as_slice().len(), expected.len());
        for (diagnostic, (path, line)) in first.as_slice().iter().zip(expected) {
            assert_eq!(
                (diagnostic.code(), diagnostic.path().as_str()),
                ("BXG0024", path)
            );
            assert_eq!(diagnostic.span(), span((line, 13), (line, 21)));
            assert_eq!(
                diagnostic.offending_construct(),
                "invalid contract declaration annotation"
            );
            assert_eq!(diagnostic.rule(), CONTRACT_ROLE_RULE);
            assert_eq!(diagnostic.rule_source(), CONTRACT_ROLE_RULE_SOURCE);
        }
        let rendered = format!("{first}\n{first:?}");
        for private in [
            "BadStruct",
            "PrivateMany",
            "PrivatePath",
            "PrivateNested",
            "PrivateType",
            "PrivateLiteral",
            "PrivateMarker",
            "PrivateVariant",
        ] {
            assert!(!rendered.contains(private));
        }
    }

    #[test]
    fn every_earlier_phase_suppresses_contract_role_validation() {
        let syntax = parse_errors(&request(
            "root.rs",
            &[
                ("root.rs", "#[boxology::contract(private)] struct Secret;"),
                ("broken.rs", "fn broken() { @ }"),
            ],
        ));
        assert!(
            syntax
                .as_slice()
                .iter()
                .all(|diagnostic| diagnostic.code() == "BXG0014")
        );

        let suppressed = |code, files: &[(&str, &str)]| {
            let diagnostics = discovery_errors(&request("root.rs", files));
            assert!(
                diagnostics
                    .as_slice()
                    .iter()
                    .all(|diagnostic| diagnostic.code() == code)
            );
            assert!(!diagnostics.to_string().contains("BXG0024"));
        };
        suppressed(
            "BXG0016",
            &[(
                "root.rs",
                "#[path = \"x.rs\"] mod x; #[boxology::contract(private)] struct S;",
            )],
        );
        suppressed(
            "BXG0017",
            &[(
                "root.rs",
                "mod missing; #[boxology::contract(private)] struct S;",
            )],
        );
        suppressed(
            "BXG0018",
            &[
                (
                    "root.rs",
                    "mod both; #[boxology::contract(private)] struct S;",
                ),
                ("both.rs", ""),
                ("both/mod.rs", ""),
            ],
        );
        suppressed(
            "BXG0019",
            &[
                ("root.rs", ""),
                ("dead.rs", "#[boxology::contract(private)] struct S;"),
            ],
        );
        suppressed(
            "BXG0020",
            &[(
                "root.rs",
                "#[cfg(private)] #[boxology::contract(private)] struct S;",
            )],
        );
        suppressed(
            "BXG0021",
            &[(
                "root.rs",
                "#[boxology::contract(private)] struct S; #[boxology::contract(private)] enum S { A }",
            )],
        );
        suppressed(
            "BXG0022",
            &[(
                "root.rs",
                "#[Private] #[boxology::contract(private)] struct S;",
            )],
        );
        suppressed(
            "BXG0023",
            &[(
                "root.rs",
                "#[derive(Copy)] #[boxology::contract(private)] struct S;",
            )],
        );
    }

    #[test]
    fn deprecations_accept_exact_forms_at_every_owned_site() {
        let source = concat!(
            "#[deprecated]\n#[boxology::contract]\n",
            "struct Bare { #[deprecated(note = r#\"PrivateNamed\"#,)] r#named: &'static [u8; 7], plain: u16 }\n",
            "#[r#deprecated(r#note = r##\"PrivateRaw\"##,)]\n#[boxology::contract]\n",
            "struct Tuple(#[deprecated] u8, u16);\n",
            "#[deprecated]\n#[boxology::contract]\nstruct Unit;\n",
            "#[deprecated(note = \"PrivateEnum\")]\n#[boxology::contract(error)]\nenum Event {\n",
            "#[deprecated] r#Unit,\n",
            "#[deprecated(note = \"PrivateTupleVariant\")] Tuple(#[deprecated] u8),\n",
            "Named { #[deprecated(note = \"PrivateNamedVariantField\")] r#value: u8 },\n}\n",
            "#[deprecated(PrivateInternal)] struct Internal;\n",
        );
        let parsed = ParsedRustInputs::parse(&request("root.rs", &[("root.rs", source)])).unwrap();
        let declarations = parsed.discover_contract_declarations().unwrap();
        let [bare, tuple, unit, event] = declarations.as_slice() else {
            panic!()
        };
        assert_eq!(bare.metadata().deprecation().unwrap().note(), None);
        let ContractDeclarationShape::Struct(ContractFields::Named(fields)) = bare.shape() else {
            panic!()
        };
        assert_eq!(
            fields
                .iter()
                .map(ContractField::ordinal)
                .collect::<Vec<_>>(),
            [0, 1]
        );
        assert_eq!(
            fields
                .iter()
                .map(|field| field.identity().unwrap().name())
                .collect::<Vec<_>>(),
            ["named", "plain"]
        );
        let identity = fields[0].identity().unwrap();
        assert!(std::ptr::eq(
            identity.ident(),
            fields[0].syntax().ident.as_ref().unwrap()
        ));
        let syn::Type::Reference(reference) = fields[0].ty() else {
            panic!()
        };
        assert!(matches!(reference.elem.as_ref(), syn::Type::Array(_)));
        assert_eq!(
            fields[0].metadata().deprecation().unwrap().note(),
            Some("PrivateNamed")
        );
        assert!(fields[1].metadata().deprecation().is_none());
        let ContractDeclarationShape::Struct(ContractFields::Unnamed(fields)) = tuple.shape()
        else {
            panic!()
        };
        assert_eq!(
            fields
                .iter()
                .map(ContractField::ordinal)
                .collect::<Vec<_>>(),
            [0, 1]
        );
        assert!(fields.iter().all(|field| field.identity().is_none()));
        assert_eq!(fields[0].metadata().deprecation().unwrap().note(), None);
        assert!(fields[1].metadata().deprecation().is_none());
        assert_eq!(
            tuple.metadata().deprecation().unwrap().note(),
            Some("PrivateRaw")
        );
        assert!(matches!(
            unit.shape(),
            ContractDeclarationShape::Struct(ContractFields::Unit)
        ));
        assert_eq!(unit.metadata().deprecation().unwrap().note(), None);
        let ContractDeclarationShape::Enum(variants) = event.shape() else {
            panic!()
        };
        assert_eq!(
            event.metadata().deprecation().unwrap().note(),
            Some("PrivateEnum")
        );
        assert_eq!(
            variants.iter().map(|v| v.ordinal()).collect::<Vec<_>>(),
            [0, 1, 2]
        );
        assert_eq!(
            variants
                .iter()
                .map(|v| v.identity().name())
                .collect::<Vec<_>>(),
            ["Unit", "Tuple", "Named"]
        );
        assert!(matches!(variants[0].fields(), ContractFields::Unit));
        assert_eq!(variants[0].metadata().deprecation().unwrap().note(), None);
        assert_eq!(
            variants[1].metadata().deprecation().unwrap().note(),
            Some("PrivateTupleVariant")
        );
        let ContractDeclarationSyntax::Enum(item) = event.syntax() else {
            panic!()
        };
        assert!(std::ptr::eq(variants[0].syntax(), &item.variants[0]));
        let ContractFields::Unnamed(fields) = variants[1].fields() else {
            panic!()
        };
        assert!(fields[0].identity().is_none());
        assert_eq!(fields[0].metadata().deprecation().unwrap().note(), None);
        let ContractFields::Named(fields) = variants[2].fields() else {
            panic!()
        };
        assert!(variants[2].metadata().deprecation().is_none());
        assert_eq!(fields[0].identity().unwrap().name(), "value");
        assert_eq!(
            fields[0].metadata().deprecation().unwrap().note(),
            Some("PrivateNamedVariantField")
        );
    }

    #[test]
    fn documentation_is_decoded_ordered_exact_and_uniform_with_deprecation() {
        let source = concat!(
            "/// declaration comment\n#[doc = \"escaped\\nline\"]\n#[r#doc = r#\" raw\\ntext \"#]\n",
            "#[deprecated]\n#[boxology::contract]\nstruct Named {\n",
            "#[doc = \" named \" ] #[deprecated(note = \"named note\")] named: u8,\n",
            "#[doc = \"\"] plain: u16,\n}\n",
            "#[doc = \"tuple\"] #[deprecated(note = \"tuple note\")] #[boxology::contract]\n",
            "struct Tuple(#[doc = \"tuple\nfield\"] #[deprecated] u8, u16);\n",
            "#[boxology::contract] struct Unit;\n",
            "#[doc = \"enum\"] #[deprecated(note = \"enum note\")] #[boxology::contract(error)]\n",
            "enum Event {\n#[doc = \"unit variant\"] #[deprecated] Unit,\n",
            "#[doc = \"tuple variant\"] Tuple(#[r#doc = r##\" raw variant field \"##] #[deprecated(note = \"field note\")] u8),\n",
            "#[doc = \"named variant\"] Named { #[doc = \"named variant\\nfield\"] value: u8 },\n}\n",
        );
        let parsed = ParsedRustInputs::parse(&request("root.rs", &[("root.rs", source)])).unwrap();
        let declarations = parsed.discover_contract_declarations().unwrap();
        let [named, tuple, unit, event] = declarations.as_slice() else {
            panic!()
        };
        assert_eq!(
            named.metadata().docs(),
            [" declaration comment", "escaped\nline", " raw\\ntext "]
        );
        assert_eq!(named.metadata().deprecation().unwrap().note(), None);
        let ContractDeclarationShape::Struct(ContractFields::Named(fields)) = named.shape() else {
            panic!()
        };
        assert_eq!(fields[0].metadata().docs(), [" named "]);
        assert_eq!(
            fields[0].metadata().deprecation().unwrap().note(),
            Some("named note")
        );
        assert_eq!(fields[1].metadata().docs(), [""]);
        let ContractDeclarationShape::Struct(ContractFields::Unnamed(fields)) = tuple.shape()
        else {
            panic!()
        };
        assert_eq!(tuple.metadata().docs(), ["tuple"]);
        assert_eq!(
            tuple.metadata().deprecation().unwrap().note(),
            Some("tuple note")
        );
        assert_eq!(fields[0].metadata().docs(), ["tuple\nfield"]);
        assert_eq!(fields[0].metadata().deprecation().unwrap().note(), None);
        assert!(fields[1].metadata().docs().is_empty());
        assert!(unit.metadata().docs().is_empty());
        assert!(unit.metadata().deprecation().is_none());
        let ContractDeclarationShape::Enum(variants) = event.shape() else {
            panic!()
        };
        assert_eq!(event.metadata().docs(), ["enum"]);
        assert_eq!(
            event.metadata().deprecation().unwrap().note(),
            Some("enum note")
        );
        assert_eq!(variants[0].metadata().docs(), ["unit variant"]);
        assert_eq!(variants[0].metadata().deprecation().unwrap().note(), None);
        assert_eq!(variants[1].metadata().docs(), ["tuple variant"]);
        let ContractFields::Unnamed(fields) = variants[1].fields() else {
            panic!()
        };
        assert_eq!(fields[0].metadata().docs(), [" raw variant field "]);
        assert_eq!(
            fields[0].metadata().deprecation().unwrap().note(),
            Some("field note")
        );
        assert_eq!(variants[2].metadata().docs(), ["named variant"]);
        let ContractFields::Named(fields) = variants[2].fields() else {
            panic!()
        };
        assert_eq!(fields[0].metadata().docs(), ["named variant\nfield"]);
    }

    #[test]
    fn invalid_deprecations_are_exact_complete_repeatable_and_payload_safe() {
        let files = [
            ("root.rs", "mod z; mod a;\n"),
            (
                "a.rs",
                concat!(
                    "#[deprecated()] #[boxology::contract] struct PrivateEmpty;\n",
                    "#[deprecated(note)] #[boxology::contract] struct PrivateBareNote;\n",
                    "#[deprecated = \"PrivateAssigned\"] #[boxology::contract] struct PrivateAssignment;\n",
                    "#[deprecated(since = \"PrivateSince\")] #[boxology::contract] struct PrivateSinceType;\n",
                    "#[deprecated(note = \"PrivateOne\", since = \"PrivateTwo\")] #[boxology::contract] struct PrivateMany;\n",
                    "#[deprecated(note = \"PrivateOne\", note = \"PrivateTwo\")] #[boxology::contract] struct PrivateRepeated;\n",
                ),
            ),
            (
                "z.rs",
                concat!(
                    "#[deprecated(PrivatePath::note = \"PrivateQualified\")] #[boxology::contract] struct PrivateQualifiedType;\n",
                    "#[deprecated(note::<PrivateType> = \"PrivateParameterized\")] #[boxology::contract] struct PrivateParameterizedType;\n",
                    "#[deprecated(note = 7)] #[boxology::contract] struct PrivateNumber;\n",
                    "#[deprecated(note = concat!(\"PrivateComputed\"))] #[boxology::contract] struct PrivateComputedType;\n",
                    "#[deprecated(note = #[PrivateExprAttr] \"PrivateAttributed\")] #[boxology::contract] struct PrivateAttributedType;\n",
                    "#[deprecated(note => \"PrivateMalformed\")] #[boxology::contract] struct PrivateMalformedType;\n",
                    "#[deprecated(note)] #[deprecated(since = \"PrivateDuplicate\")] #[boxology::contract] struct PrivateDuplicateType;\n",
                ),
            ),
        ];
        let first = discovery_errors(&request_in_order("root.rs", &files, false));
        assert_eq!(
            first,
            discovery_errors(&request_in_order("root.rs", &files, false))
        );
        assert_eq!(
            first,
            discovery_errors(&request_in_order("root.rs", &files, true))
        );
        let expected = [
            ("a.rs", 1, 3),
            ("a.rs", 2, 3),
            ("a.rs", 3, 3),
            ("a.rs", 4, 3),
            ("a.rs", 5, 3),
            ("a.rs", 6, 3),
            ("z.rs", 1, 3),
            ("z.rs", 2, 3),
            ("z.rs", 3, 3),
            ("z.rs", 4, 3),
            ("z.rs", 5, 3),
            ("z.rs", 6, 3),
            ("z.rs", 7, 3),
            ("z.rs", 7, 23),
        ];
        assert_eq!(first.as_slice().len(), expected.len());
        for (diagnostic, (path, line, column)) in first.as_slice().iter().zip(expected) {
            assert_eq!(
                (diagnostic.code(), diagnostic.path().as_str()),
                ("BXG0025", path)
            );
            assert_eq!(diagnostic.span(), span((line, column), (line, column + 10)));
            assert_eq!(
                diagnostic.offending_construct(),
                "invalid or duplicate deprecation attribute"
            );
            assert_eq!(diagnostic.rule(), DEPRECATION_RULE);
            assert_eq!(diagnostic.rule_source(), DEPRECATION_RULE_SOURCE);
        }
        assert!(first.as_slice().windows(2).all(|pair| pair[0] <= pair[1]));
        let rendered = format!("{first}\n{first:?}");
        for private in [
            "PrivateEmpty",
            "PrivateBareNote",
            "PrivateAssigned",
            "PrivateAssignment",
            "PrivateSince",
            "PrivateOne",
            "PrivateTwo",
            "PrivateMany",
            "PrivateRepeated",
            "PrivatePath",
            "PrivateQualified",
            "PrivateType",
            "PrivateParameterized",
            "PrivateNumber",
            "PrivateComputed",
            "PrivateExprAttr",
            "PrivateAttributed",
            "PrivateMalformed",
            "PrivateDuplicate",
        ] {
            assert!(!rendered.contains(private));
        }
    }

    #[test]
    fn every_earlier_phase_suppresses_deprecation_validation() {
        let syntax = parse_errors(&request(
            "root.rs",
            &[
                (
                    "root.rs",
                    "#[doc] #[deprecated(Private)] #[boxology::contract] struct S;",
                ),
                ("broken.rs", "fn broken() { @ }"),
            ],
        ));
        assert!(
            syntax
                .as_slice()
                .iter()
                .all(|diagnostic| diagnostic.code() == "BXG0014")
        );

        let suppressed = |code, files: &[(&str, &str)]| {
            let diagnostics = discovery_errors(&request("root.rs", files));
            assert!(
                diagnostics
                    .as_slice()
                    .iter()
                    .all(|diagnostic| diagnostic.code() == code)
            );
            assert!(!diagnostics.to_string().contains("BXG0025"));
            assert!(!diagnostics.to_string().contains("BXG0026"));
        };
        suppressed(
            "BXG0016",
            &[(
                "root.rs",
                "#[path = \"x.rs\"] mod x; #[doc] #[deprecated(Private)] #[boxology::contract] struct S;",
            )],
        );
        suppressed(
            "BXG0017",
            &[(
                "root.rs",
                "mod missing; #[doc] #[deprecated(Private)] #[boxology::contract] struct S;",
            )],
        );
        suppressed(
            "BXG0018",
            &[
                (
                    "root.rs",
                    "mod both; #[doc] #[deprecated(Private)] #[boxology::contract] struct S;",
                ),
                ("both.rs", ""),
                ("both/mod.rs", ""),
            ],
        );
        suppressed(
            "BXG0019",
            &[
                ("root.rs", ""),
                (
                    "dead.rs",
                    "#[doc] #[deprecated(Private)] #[boxology::contract] struct S;",
                ),
            ],
        );
        suppressed(
            "BXG0020",
            &[(
                "root.rs",
                "#[cfg(Private)] #[doc] #[deprecated(Private)] #[boxology::contract] struct S;",
            )],
        );
        suppressed(
            "BXG0021",
            &[(
                "root.rs",
                "#[doc] #[deprecated(Private)] #[boxology::contract] struct S; #[boxology::contract] enum S { A }",
            )],
        );
        suppressed(
            "BXG0022",
            &[(
                "root.rs",
                "#[::deprecated(Private)] #[doc] #[boxology::contract] struct S;",
            )],
        );
        suppressed(
            "BXG0023",
            &[(
                "root.rs",
                "#[derive(Copy)] #[doc] #[deprecated(Private)] #[boxology::contract] struct S;",
            )],
        );
        suppressed(
            "BXG0024",
            &[(
                "root.rs",
                "#[doc] #[deprecated(Private)] #[boxology::contract(Private)] struct S;",
            )],
        );
    }

    #[test]
    fn invalid_documentation_is_complete_exact_deterministic_and_payload_safe() {
        let files = [
            ("root.rs", "mod z; mod a;\n"),
            (
                "a.rs",
                concat!(
                    "#[doc]\n#[boxology::contract] struct PrivatePath;\n",
                    "#[r#doc(PrivateRawList)]\n#[boxology::contract] struct PrivateList;\n",
                    "#[boxology::contract] struct PrivateFields { #[doc = 7] number: u8 }\n",
                ),
            ),
            (
                "z.rs",
                concat!(
                    "#[boxology::contract] enum PrivateEvent {\n",
                    "#[doc = concat!(\"PrivateMacro\")] Macro,\n",
                    "#[doc = (\"PrivateParen\")] Tuple(u8),\n",
                    "#[doc = PrivateComputed] Named { #[doc = { \"PrivateBlock\" }] value: u8 },\n}\n",
                ),
            ),
        ];
        let first = discovery_errors(&request_in_order("root.rs", &files, false));
        assert_eq!(
            first,
            discovery_errors(&request_in_order("root.rs", &files, false))
        );
        assert_eq!(
            first,
            discovery_errors(&request_in_order("root.rs", &files, true))
        );
        assert_eq!(first.as_slice().len(), 7);
        assert!(first.as_slice().windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(first.as_slice()[0].span(), span((1, 3), (1, 6)));
        assert_eq!(first.as_slice()[1].span(), span((3, 3), (3, 8)));
        for diagnostic in first.as_slice() {
            assert_eq!(diagnostic.code(), "BXG0026");
            assert_eq!(
                diagnostic.offending_construct(),
                "invalid documentation attribute"
            );
            assert_eq!(diagnostic.rule(), DOCUMENTATION_RULE);
            assert_eq!(diagnostic.rule_source(), DOCUMENTATION_RULE_SOURCE);
        }
        // Model `#[doc = #[PrivateExpr] "PrivateAttributed"]` with a direct attributed literal.
        let mut direct = syn::Attribute::parse_outer
            .parse_str("#[doc = \"template\"]")
            .unwrap();
        let syn::Meta::NameValue(value) = &mut direct[0].meta else {
            panic!()
        };
        value.value = syn::parse_str("#[PrivateExpr] \"PrivateAttributed\"").unwrap();
        assert!(matches!(
            &value.value,
            syn::Expr::Lit(syn::ExprLit { attrs, .. }) if attrs.len() == 1
        ));
        let mut direct_diagnostics = Vec::new();
        validate_documentation(
            &RelativePath("direct.rs".into()),
            &direct,
            &mut direct_diagnostics,
        );
        assert_eq!(direct_diagnostics.len(), 1);
        assert_eq!(first.as_slice().len() + direct_diagnostics.len(), 8);
        assert_eq!(direct_diagnostics[0].code(), "BXG0026");
        assert_eq!(direct_diagnostics[0].span(), span((1, 3), (1, 6)));
        let rendered = format!("{first}\n{first:?}\n{direct_diagnostics:?}");
        for private in [
            "PrivatePath",
            "PrivateRawList",
            "PrivateList",
            "PrivateFields",
            "PrivateEvent",
            "PrivateMacro",
            "PrivateParen",
            "PrivateExpr",
            "PrivateAttributed",
            "PrivateComputed",
            "PrivateBlock",
            "concat",
        ] {
            assert!(!rendered.contains(private));
        }
        let parsed = ParsedRustInputs::parse(&request_in_order("root.rs", &files, false)).unwrap();
        assert!(parsed.discover_contract_declarations().is_err());
    }

    #[test]
    fn allowlist_and_deprecation_phase_suppress_documentation_validation() {
        let qualified = discovery_errors(&request(
            "root.rs",
            &[(
                "root.rs",
                concat!(
                    "#[::doc = \"PrivateQualified\"] #[boxology::contract] struct Qualified;\n",
                    "#[alias::doc = \"PrivateAliased\"] #[boxology::contract] struct Aliased;\n",
                ),
            )],
        ));
        assert_eq!(qualified.as_slice().len(), 2);
        assert!(
            qualified
                .as_slice()
                .iter()
                .all(|error| error.code() == "BXG0022")
        );
        let inner = syn::Attribute::parse_inner
            .parse_str("#![doc = \"PrivateInner\"]")
            .unwrap();
        let mut inner_diagnostics = Vec::new();
        validate_attributes(
            &RelativePath("inner.rs".into()),
            &inner,
            &mut inner_diagnostics,
        );
        assert_eq!(inner_diagnostics[0].code(), "BXG0022");
        let deprecation = discovery_errors(&request(
            "root.rs",
            &[(
                "root.rs",
                "#[deprecated(Private)] #[doc] #[boxology::contract] struct S;",
            )],
        ));
        assert!(
            deprecation
                .as_slice()
                .iter()
                .all(|error| error.code() == "BXG0025")
        );
        assert!(!deprecation.to_string().contains("BXG0026"));
    }

    #[test]
    fn rejected_attributes_are_owned_by_each_direct_contract_site() {
        let source = concat!(
            "#[PrivateDeclAttr(secret)]\n#[boxology::contract]\nstruct S {\n",
            "#[PrivateFieldAttr(secret)]\nvalue: u8,\n}\n",
            "#[boxology::contract]\nenum E {\n",
            "#[PrivateVariantAttr(secret)]\nA,\nB {\n",
            "#[::PrivateVariantFieldAttr(secret)]\nvalue: u8,\n}\n}\n",
        );
        let diagnostics = discovery_errors(&request("root.rs", &[("root.rs", source)]));
        let expected = [
            span((1, 3), (1, 18)),
            span((4, 3), (4, 19)),
            span((9, 3), (9, 21)),
            span((12, 3), (12, 28)),
        ];
        assert_eq!(diagnostics.as_slice().len(), expected.len());
        for (diagnostic, expected) in diagnostics.as_slice().iter().zip(expected) {
            assert_eq!(diagnostic.code(), "BXG0022");
            assert_eq!(diagnostic.span(), expected);
            assert_eq!(
                diagnostic.offending_construct(),
                "non-allowlisted contract attribute"
            );
            assert_eq!(diagnostic.rule(), ATTRIBUTE_RULE);
            assert_eq!(diagnostic.rule_source(), RULE_SOURCE);
        }
        let output = format!("{diagnostics}\n{diagnostics:?}");
        for private in [
            "PrivateDeclAttr",
            "PrivateFieldAttr",
            "PrivateVariantAttr",
            "PrivateVariantFieldAttr",
            "secret",
        ] {
            assert!(!output.contains(private));
        }
    }

    #[test]
    fn derive_allowlist_is_exact_complete_and_payload_safe() {
        let source = concat!(
            "#[boxology::contract]\n#[derive(Debug, r#Clone, PartialEq)]\n#[derive()]\n",
            "#[derive(Debug, serde::Serialize, Clone)]\n#[derive(::PartialEq)]\n",
            "#[derive(Debug<u8>)]\n#[derive(Debug(u8))]\n#[derive(Fn() -> SecretReturn)]\n",
            "#[derive(Copy)]\n#[derive]\n#[derive = \"PrivateValue\"]\nstruct Bad;\n",
        );
        let diagnostics = discovery_errors(&request("root.rs", &[("root.rs", source)]));
        let expected = [
            span((4, 17), (4, 33)),
            span((5, 10), (5, 21)),
            span((6, 10), (6, 19)),
            span((7, 10), (7, 19)),
            span((8, 10), (8, 30)),
            span((9, 10), (9, 14)),
            span((10, 3), (10, 9)),
            span((11, 3), (11, 9)),
        ];
        assert_eq!(diagnostics.as_slice().len(), expected.len());
        for (diagnostic, expected) in diagnostics.as_slice().iter().zip(expected) {
            assert_eq!(diagnostic.code(), "BXG0023");
            assert_eq!(diagnostic.span(), expected);
            assert_eq!(
                diagnostic.offending_construct(),
                "non-allowlisted contract derive"
            );
            assert_eq!(diagnostic.rule(), DERIVE_RULE);
        }
        let output = format!("{diagnostics}\n{diagnostics:?}");
        for private in [
            "serde",
            "Serialize",
            "SecretReturn",
            "Copy",
            "PrivateValue",
            "u8",
        ] {
            assert!(!output.contains(private));
        }
    }

    #[test]
    fn earlier_phases_suppress_allowlist_and_findings_are_input_order_invariant() {
        let collision = discovery_errors(&request(
            "root.rs",
            &[(
                "root.rs",
                "#[boxology::contract]\n#[PrivateOne]\nstruct Foo;\n#[boxology::contract]\n#[PrivateTwo]\nenum Foo { A }",
            )],
        ));
        assert_eq!(collision.as_slice().len(), 1);
        assert_eq!(collision.as_slice()[0].code(), "BXG0021");
        for (source, code) in [
            (
                "#[boxology::contract]\n#[Private]\nstruct Unique;\nmod missing;",
                "BXG0017",
            ),
            (
                "#[cfg(secret)]\n#[boxology::contract]\n#[Private]\nstruct Unique;",
                "BXG0020",
            ),
        ] {
            let diagnostics = discovery_errors(&request("root.rs", &[("root.rs", source)]));
            assert_eq!(diagnostics.as_slice()[0].code(), code);
            assert!(
                diagnostics
                    .as_slice()
                    .iter()
                    .all(|diagnostic| !matches!(diagnostic.code(), "BXG0022" | "BXG0023"))
            );
        }

        let files = [
            ("root.rs", "mod z; mod a;"),
            ("z.rs", "#[boxology::contract]\n#[Private]\nstruct Z;"),
            ("a.rs", "#[boxology::contract]\n#[derive(Copy)]\nstruct A;"),
        ];
        let first = discovery_errors(&request("root.rs", &files));
        let reversed = files.into_iter().rev().collect::<Vec<_>>();
        assert_eq!(first, discovery_errors(&request("root.rs", &reversed)));
        assert_eq!(
            first
                .as_slice()
                .iter()
                .map(|diagnostic| (diagnostic.path().as_str(), diagnostic.code()))
                .collect::<Vec<_>>(),
            [("a.rs", "BXG0023"), ("z.rs", "BXG0022")]
        );
    }

    #[test]
    fn earlier_structural_and_conditional_phases_suppress_collisions() {
        for (source, code) in [
            (
                "#[boxology::contract] struct Foo;\n#[boxology::contract] enum Foo { A }\nmod missing;",
                "BXG0017",
            ),
            (
                "#[cfg(secret)]\n#[boxology::contract] struct Foo;\n#[boxology::contract] enum Foo { A }",
                "BXG0020",
            ),
        ] {
            let diagnostics = discovery_errors(&request("root.rs", &[("root.rs", source)]));
            assert_eq!(diagnostics.as_slice().len(), 1);
            assert_eq!(diagnostics.as_slice()[0].code(), code);
            assert!(!diagnostics.to_string().contains("BXG0021"));
        }
    }

    #[test]
    fn member_identity_namespaces_accept_distinct_and_provisional_shapes() {
        let source = concat!(
            "#[boxology::contract]\n",
            "struct First { same: u8, Field: u8, field: u8, café: u8, 東京: u8 }\n",
            "#[boxology::contract]\nstruct Second { same: u8 }\n",
            "#[boxology::contract]\nstruct Tuple(u8, u16);\n",
            "#[boxology::contract]\nstruct Unit;\n",
            "#[boxology::contract(error)]\nenum Event {\n",
            "same,\nField { same: u8, café: u8 },\n",
            "field { same: u16, 東京: u8 },\nTuple(u8),\nUnit,\n}\n",
        );
        let parsed = ParsedRustInputs::parse(&request("root.rs", &[("root.rs", source)])).unwrap();
        let declarations = parsed.discover_contract_declarations().unwrap();
        let [first, second, tuple, unit, event] = declarations.as_slice() else {
            panic!()
        };
        let named = |declaration: &ContractDeclaration<'_>| {
            let ContractDeclarationShape::Struct(ContractFields::Named(fields)) =
                declaration.shape()
            else {
                panic!()
            };
            fields
                .iter()
                .map(|field| field.identity().unwrap().name().to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(named(first), ["same", "Field", "field", "café", "東京"]);
        assert_eq!(named(second), ["same"]);
        assert!(matches!(
            tuple.shape(),
            ContractDeclarationShape::Struct(ContractFields::Unnamed(_))
        ));
        assert!(matches!(
            unit.shape(),
            ContractDeclarationShape::Struct(ContractFields::Unit)
        ));
        let ContractDeclarationShape::Enum(variants) = event.shape() else {
            panic!()
        };
        assert_eq!(
            variants
                .iter()
                .map(|variant| variant.identity().name())
                .collect::<Vec<_>>(),
            ["same", "Field", "field", "Tuple", "Unit"]
        );
        for variant in &variants[1..=2] {
            let ContractFields::Named(fields) = variant.fields() else {
                panic!()
            };
            assert_eq!(fields[0].identity().unwrap().name(), "same");
        }
    }

    #[test]
    fn duplicate_member_identities_use_raw_unspelled_source_order_and_exact_spans() {
        let source = concat!(
            "#[boxology::contract]\nstruct Duplicates {\n",
            "name: u8,\nr#name: u8,\ntrio: u8,\ntrio: u8,\ntrio: u8,\n",
            "café: u8,\ncafé: u8,\n}\n",
            "#[boxology::contract]\nenum Events {\n",
            "Variant,\nr#Variant,\nTrio,\nTrio,\nTrio,\n",
            "Named {\nfield: u8,\nfield: u8,\n}\n}\n",
        );
        let diagnostics = discovery_errors(&request("root.rs", &[("root.rs", source)]));
        let expected = [
            ("BXG0027", 4, 7),
            ("BXG0027", 6, 5),
            ("BXG0027", 7, 5),
            ("BXG0027", 9, 5),
            ("BXG0028", 14, 10),
            ("BXG0028", 16, 5),
            ("BXG0028", 17, 5),
            ("BXG0027", 20, 6),
        ];
        assert_eq!(diagnostics.as_slice().len(), expected.len());
        for (diagnostic, (code, line, end)) in diagnostics.as_slice().iter().zip(expected) {
            assert_eq!(diagnostic.code(), code);
            assert_eq!(diagnostic.span(), span((line, 1), (line, end)));
            assert_eq!(diagnostic.rule_source(), MEMBER_IDENTITY_RULE_SOURCE);
            if code == "BXG0027" {
                assert_eq!(
                    diagnostic.offending_construct(),
                    "duplicate named contract field identity"
                );
                assert_eq!(diagnostic.rule(), FIELD_IDENTITY_RULE);
            } else {
                assert_eq!(
                    diagnostic.offending_construct(),
                    "duplicate contract enum variant identity"
                );
                assert_eq!(diagnostic.rule(), VARIANT_IDENTITY_RULE);
            }
        }
        assert!(
            diagnostics
                .as_slice()
                .windows(2)
                .all(|pair| pair[0] < pair[1])
        );
    }

    #[test]
    fn member_identity_findings_are_complete_deterministic_deduplicated_and_safe() {
        let files = [
            (
                "root.rs",
                "mod a; mod z;\n#[boxology::contract]\nstruct RootPayload {\nRootSecret: PrivateScalar,\nRootSecret: PrivateScalar,\n}\n",
            ),
            (
                "a.rs",
                "#[boxology::contract]\nenum EnvelopePayload {\nVariantSecret {\nFieldSecret: PrivateScalar,\nFieldSecret: PrivateScalar,\n},\nVariantSecret,\n}\n",
            ),
            (
                "z.rs",
                "#[boxology::contract]\nstruct UnicodePayload {\n東京秘密: PrivateScalar,\n東京秘密: PrivateScalar,\n}\n",
            ),
        ];
        let canonical = request_in_order("root.rs", &files, false);
        let first = discovery_errors(&canonical);
        assert_eq!(first, discovery_errors(&canonical));
        assert_eq!(
            first,
            discovery_errors(&request_in_order("root.rs", &files, true))
        );
        assert_eq!(
            first
                .as_slice()
                .iter()
                .map(|diagnostic| (
                    diagnostic.path().as_str(),
                    diagnostic.span().start().line(),
                    diagnostic.code(),
                ))
                .collect::<Vec<_>>(),
            [
                ("a.rs", 5, "BXG0027"),
                ("a.rs", 7, "BXG0028"),
                ("root.rs", 5, "BXG0027"),
                ("z.rs", 4, "BXG0027"),
            ]
        );
        assert!(first.as_slice().windows(2).all(|pair| pair[0] < pair[1]));
        let parsed = ParsedRustInputs::parse(&canonical).unwrap();
        assert!(parsed.discover_contract_declarations().is_err());
        let rendered = format!("{first}\n{first:?}");
        for private in [
            "RootPayload",
            "RootSecret",
            "EnvelopePayload",
            "VariantSecret",
            "FieldSecret",
            "UnicodePayload",
            "東京秘密",
            "PrivateScalar",
        ] {
            assert!(!rendered.contains(private));
        }
    }

    #[test]
    fn earlier_phases_suppress_member_identity_validation() {
        for (source, code) in [
            (
                "#[Private]\n#[boxology::contract]\nstruct S { duplicate: u8, duplicate: u8 }\n#[boxology::contract]\nenum E { Duplicate, Duplicate }",
                "BXG0022",
            ),
            (
                "#[doc]\n#[boxology::contract]\nstruct S { duplicate: u8, duplicate: u8 }\n#[boxology::contract]\nenum E { Duplicate, Duplicate }",
                "BXG0026",
            ),
        ] {
            let diagnostics = discovery_errors(&request("root.rs", &[("root.rs", source)]));
            assert!(
                diagnostics
                    .as_slice()
                    .iter()
                    .all(|diagnostic| diagnostic.code() == code)
            );
            let rendered = diagnostics.to_string();
            assert!(!rendered.contains("BXG0027"));
            assert!(!rendered.contains("BXG0028"));
        }
    }

    #[test]
    fn module_resolution_diagnostics_are_exact_and_safe() {
        let cases = [
            (
                &[(
                    "root.rs",
                    "#[path = \"never-print-this.rs\"] mod redirected;\n",
                )][..],
                "BXG0016",
                span((1, 3), (1, 7)),
                "module path override",
                PATH_RULE,
            ),
            (
                &[("root.rs", "mod missing;\n")][..],
                "BXG0017",
                span((1, 5), (1, 12)),
                "missing outline module input",
                MISSING_RULE,
            ),
            (
                &[
                    ("duplicate/mod.rs", ""),
                    ("root.rs", "mod duplicate;\n"),
                    ("duplicate.rs", ""),
                ][..],
                "BXG0018",
                span((1, 5), (1, 14)),
                "ambiguous outline module inputs",
                AMBIGUOUS_RULE,
            ),
        ];
        for (files, code, expected_span, offending, rule) in cases {
            let diagnostics = resolution_errors(&request("root.rs", files));
            assert_eq!(diagnostics.as_slice().len(), 1);
            let diagnostic = &diagnostics.as_slice()[0];
            assert_eq!(diagnostic.code(), code);
            assert_eq!(diagnostic.path().as_str(), "root.rs");
            assert_eq!(diagnostic.span(), expected_span);
            assert_eq!(diagnostic.offending_construct(), offending);
            assert_eq!(diagnostic.rule(), rule);
            assert_eq!(diagnostic.rule_source(), RULE_SOURCE);
            assert_eq!(
                diagnostic.to_string(),
                format!(
                    "{code} root.rs:{}:{}-{}:{} offending={offending:?} rule={rule:?} source={RULE_SOURCE:?}",
                    expected_span.start().line(),
                    expected_span.start().column(),
                    expected_span.end().line(),
                    expected_span.end().column()
                )
            );
            for payload in [
                "never-print-this.rs",
                "redirected",
                "mod missing",
                "duplicate",
            ] {
                assert!(!diagnostic.to_string().contains(payload));
            }
        }
    }

    #[test]
    fn resolution_errors_are_complete_sorted_repeatable_and_branch_local() {
        let request = request(
            "root.rs",
            &[
                ("dead.rs", "#[boxology::contract] struct Dead;\n"),
                ("redirected_payload/mod.rs", "mod hidden_payload;\n"),
                ("ambiguous_payload.rs", "mod hidden_payload;\n"),
                ("a_continuing.rs", "mod descendant_payload;\n"),
                (
                    "root.rs",
                    "mod absent_payload;\n#[r#path = \"raw-never-print-this.rs\"]\nmod redirected_payload;\nmod ambiguous_payload;\nmod a_continuing;\n",
                ),
                ("ambiguous_payload/mod.rs", "mod hidden_payload;\n"),
                ("redirected_payload.rs", "mod hidden_payload;\n"),
            ],
        );
        let (first, second) = (resolution_errors(&request), resolution_errors(&request));
        assert_eq!(first, second);
        assert_eq!(
            first
                .as_slice()
                .iter()
                .map(|diagnostic| (
                    diagnostic.path().as_str(),
                    diagnostic.span().start().line(),
                    diagnostic.code()
                ))
                .collect::<Vec<_>>(),
            [
                ("a_continuing.rs", 1, "BXG0017"),
                ("root.rs", 1, "BXG0017"),
                ("root.rs", 2, "BXG0016"),
                ("root.rs", 4, "BXG0018"),
            ]
        );
        assert_eq!(first.as_slice()[2].span(), span((2, 3), (2, 9)));
        let rendered = first.to_string();
        for payload in [
            "never-print-this.rs",
            "raw-never-print-this.rs",
            "absent_payload",
            "redirected_payload",
            "ambiguous_payload",
            "descendant_payload",
            "hidden_payload",
        ] {
            assert!(!rendered.contains(payload));
        }
    }

    #[test]
    fn unicode_identifier_bom_and_shebang_parse_as_a_complete_file() {
        let request = request(
            "unicode.rs",
            &[(
                "unicode.rs",
                "\u{feff}#!/usr/bin/env rust-script\nfn café() {}\n",
            )],
        );
        let parsed = ParsedRustInputs::parse(&request).unwrap();
        let syntax = parsed.as_slice()[0].syntax();
        assert_eq!(
            syntax.shebang.as_deref(),
            Some("#!/usr/bin/env rust-script")
        );
        assert_eq!(syntax.items.len(), 1);
    }

    #[test]
    fn non_rust_suffix_is_ignored_while_a_valid_root_exists() {
        let request = request("root.rs", &[("notes.rs.bak", "@\n"), ("root.rs", "")]);
        let parsed = ParsedRustInputs::parse(&request).unwrap();
        assert_eq!(
            parsed
                .as_slice()
                .iter()
                .map(|input| input.path().as_str())
                .collect::<Vec<_>>(),
            ["root.rs"]
        );
    }

    #[test]
    fn multifile_failures_are_complete_sorted_exact_safe_and_repeatable() {
        let request = request(
            "root.rs",
            &[
                ("b.rs", "fn café() { @ }\n"),
                ("a.rs", "fn good() {}\nfn bad() { @ }\n"),
                ("root.rs", "fn root() {}\n"),
            ],
        );
        let (first, second) = (parse_errors(&request), parse_errors(&request));
        assert_eq!(first, second);
        let diagnostics = first.as_slice();
        assert_eq!(diagnostics.len(), 2);
        for (diagnostic, path, expected_span) in [
            (&diagnostics[0], "a.rs", span((2, 12), (2, 13))),
            (&diagnostics[1], "b.rs", span((1, 13), (1, 14))),
        ] {
            assert_eq!(diagnostic.code(), "BXG0014");
            assert_eq!(diagnostic.path().as_str(), path);
            assert_eq!(diagnostic.span(), expected_span);
            assert_eq!(diagnostic.offending_construct(), "Rust source syntax");
            assert_eq!(diagnostic.rule(), RULE);
            assert_eq!(diagnostic.rule_source(), RULE_SOURCE);
            assert_eq!(
                diagnostic.to_string(),
                format!(
                    "BXG0014 {path}:{}:{}-{}:{} offending=\"Rust source syntax\" rule=\"{RULE}\" source=\"{RULE_SOURCE}\"",
                    expected_span.start().line(),
                    expected_span.start().column(),
                    expected_span.end().line(),
                    expected_span.end().column()
                )
            );
            assert!(!diagnostic.to_string().contains(['\r', '\n']));
        }
        let display = first.to_string();
        for payload in ["good", "bad", "café", "@", "expected expression"] {
            assert!(!display.contains(payload));
        }
    }

    #[test]
    fn combined_syn_error_components_are_all_aggregated() {
        let mut error = syn::Error::new(proc_macro2::Span::call_site(), "first payload");
        error.combine(syn::Error::new(
            proc_macro2::Span::call_site(),
            "second payload",
        ));
        let mut diagnostics = Vec::new();
        append_errors(&RelativePath("combined.rs".into()), error, &mut diagnostics);
        assert_eq!(diagnostics.len(), 2);
        assert!(diagnostics.iter().all(|error| error.code() == "BXG0014"));
        assert!(!format!("{:?}", diagnostics).contains("payload"));
    }

    #[test]
    fn controlled_contract_discovery_is_cold_exact_and_fail_closed() {
        const HELLO: &str = "boxology::contract! { #[error] pub enum GreetError { EmptyName } #[capability(exposure=external)] pub async fn greet(name:String)->Result<String,GreetError>; }";
        let api = format!("{HELLO} fn body() {{ loop {{}} }}");
        let files = [
            ("root.rs", "mod api; opaque!({ boxology::contract!{} });"),
            ("api.rs", api.as_str()),
        ];
        let forward = request_in_order("root.rs", &files, false);
        let reverse = request_in_order("root.rs", &files, true);
        let forward = ParsedRustInputs::parse(&forward).unwrap();
        let reverse = ParsedRustInputs::parse(&reverse).unwrap();
        let mut other =
            ParsedRustInputs::parse(&request_in_order("root.rs", &files, false)).unwrap();
        other.box_id = BoxId::new("other").unwrap();
        let (hello, reversed, other) = (
            forward.controlled_contract().unwrap(),
            reverse.controlled_contract().unwrap(),
            other.controlled_contract().unwrap(),
        );
        assert_eq!(hello.source().as_str(), "api.rs");
        assert_eq!(hello.span(), span((1, 11), (1, 19)));
        assert_eq!(hello.model().capabilities[0].name, "greet");
        assert_eq!(
            (hello.canonical_semantic_bytes(), hello.semantic_digest()),
            (
                reversed.canonical_semantic_bytes(),
                reversed.semantic_digest()
            )
        );
        assert_ne!(hello.capability_id(), other.capability_id());
        assert_eq!(
            (hello.canonical_semantic_bytes(), hello.semantic_digest()),
            (other.canonical_semantic_bytes(), other.semantic_digest())
        );
        for (files, expected) in [
            (
                vec![("root.rs", "r#boxology::contract!{} ::boxology::contract!{}")],
                "BXG0037 root.rs:1:1-1:1 offending=\"missing controlled contract invocation\" rule=\"exact boxology::contract! invocations must appear once at reachable module scope\" source=\"specs/s2-contract-generator.md D2\"",
            ),
            (
                vec![("root.rs", &format!("{HELLO} {HELLO}"))],
                "BXG0037 root.rs:1:171-1:179 offending=\"additional controlled contract invocation\" rule=\"exact boxology::contract! invocations must appear once at reachable module scope\" source=\"specs/s2-contract-generator.md D2\"",
            ),
            (
                vec![
                    ("root.rs", "mod a; mod z;"),
                    ("a.rs", &format!("{HELLO} {HELLO}")),
                    ("z.rs", &format!("{HELLO} {HELLO}")),
                ],
                "BXG0037 a.rs:1:171-1:179 offending=\"additional controlled contract invocation\" rule=\"exact boxology::contract! invocations must appear once at reachable module scope\" source=\"specs/s2-contract-generator.md D2\"\nBXG0037 z.rs:1:11-1:19 offending=\"additional controlled contract invocation\" rule=\"exact boxology::contract! invocations must appear once at reachable module scope\" source=\"specs/s2-contract-generator.md D2\"\nBXG0037 z.rs:1:171-1:179 offending=\"additional controlled contract invocation\" rule=\"exact boxology::contract! invocations must appear once at reachable module scope\" source=\"specs/s2-contract-generator.md D2\"",
            ),
            (
                vec![("root.rs", &format!("fn f() {{ {HELLO} }}"))],
                "BXG0036 root.rs:1:20-1:28 offending=\"misplaced controlled contract invocation\" rule=\"exact boxology::contract! invocations must appear once at reachable module scope\" source=\"specs/s2-contract-generator.md D2\"",
            ),
            (
                vec![("root.rs", ""), ("dead.rs", HELLO)],
                "BXG0019 dead.rs:1:11-1:19 offending=\"Boxology contract invocation\" rule=\"Boxology-annotated items must be reachable from the declared crate root\" source=\"specs/s2-contract-generator.md D2\"",
            ),
            (
                vec![("root.rs", &format!("#[cfg(x)] {HELLO}"))],
                "BXG0020 root.rs:1:3-1:6 offending=\"cfg attribute\" rule=\"cfg and cfg_attr are forbidden on exported items, their fields or variants, surrounding impls, and ancestor module declarations\" source=\"specs/s2-contract-generator.md D2\"",
            ),
            (
                vec![("root.rs", "boxology::contract! { private }")],
                "BXG0038 root.rs:1:23-1:30 offending=\"invalid controlled contract syntax\" rule=\"contract tokens must satisfy the controlled v0 grammar\" source=\"specs/s2-contract-generator.md D3\"",
            ),
        ] {
            let failure = |reversed| {
                ParsedRustInputs::parse(&request_in_order("root.rs", &files, reversed))
                    .unwrap()
                    .controlled_contract()
                    .err()
                    .expect("failure case")
            };
            let diagnostics = failure(false);
            assert_eq!(diagnostics, failure(true));
            assert_eq!(diagnostics.to_string(), expected);
        }
    }
}
