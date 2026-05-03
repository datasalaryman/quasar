//! Resolution pipeline: transforms a DescriptorRegistry into canonical IDL
//! JSON.
//!
//! 5-phase pipeline:
//! 1. Collect — gather descriptors (already done by the time resolve is called)
//! 2. Resolve — type paths + generics resolution
//! 3. Lower + accounts — produce flat Program IR, topological sort account
//!    graph
//! 4. Validate — all validation rules
//! 5. Canonicalize + serialize — deterministic ordering, hashing, emit JSON

use crate::{diagnostics::DiagnosticSink, ir::registry::DescriptorRegistry};

/// The resolved Program IR — output of the resolution pipeline.
/// This is consumed by the canonicalization phase to produce IDL JSON.
#[derive(Debug)]
pub struct ProgramIR {
    pub name: String,
    pub address: String,
    pub version: String,
    // Resolved types, instructions, accounts, etc. will be populated
    // as the resolution pipeline is implemented.
}

/// Run the full resolution pipeline.
///
/// Takes a populated DescriptorRegistry and produces either a valid ProgramIR
/// or accumulated diagnostics.
pub fn resolve(
    registry: &DescriptorRegistry,
) -> Result<ProgramIR, Vec<crate::diagnostics::Diagnostic>> {
    let mut sink = DiagnosticSink::default();

    // Phase 1: Collect (already done — registry is populated)

    // Phase 2: Resolve type paths + generics
    // TODO: Implement type resolution

    // Phase 3: Lower + resolve account dependency graph
    // TODO: Implement lowering and account topological sort

    // Phase 4: Validate
    validate(registry, &mut sink);

    // Check for errors before proceeding
    if sink.has_errors() {
        return Err(sink.finish().unwrap_err());
    }

    // Phase 5: Canonicalize (happens during serialization)
    // For now, construct a minimal ProgramIR from the first program descriptor.
    let program = registry.programs.first().ok_or_else(|| {
        let mut s = DiagnosticSink::default();
        s.error("No program descriptor found");
        s.finish().unwrap_err()
    })?;

    Ok(ProgramIR {
        name: program.name.clone(),
        address: program.address.clone(),
        version: program.version.clone(),
    })
}

/// Validation phase — checks structural invariants.
fn validate(registry: &DescriptorRegistry, sink: &mut DiagnosticSink) {
    // Check discriminator collisions within each namespace.
    check_discriminator_collisions(registry, sink);
}

fn check_discriminator_collisions(registry: &DescriptorRegistry, sink: &mut DiagnosticSink) {
    use std::collections::HashMap;

    // Instruction discriminators
    let mut ix_discs: HashMap<&[u8], &str> = HashMap::new();
    for ix in &registry.instructions {
        if let Some(existing) = ix_discs.get(ix.discriminator.as_slice()) {
            sink.error(format!(
                "Discriminator collision: instructions '{}' and '{}' share discriminator {:?}",
                existing, ix.name, ix.discriminator
            ));
        } else {
            ix_discs.insert(&ix.discriminator, &ix.name);
        }
    }

    // Account discriminators
    let mut acc_discs: HashMap<&[u8], &str> = HashMap::new();
    for acc in &registry.account_data {
        if let Some(existing) = acc_discs.get(acc.discriminator.as_slice()) {
            sink.error(format!(
                "Discriminator collision: accounts '{}' and '{}' share discriminator {:?}",
                existing, acc.name, acc.discriminator
            ));
        } else {
            acc_discs.insert(&acc.discriminator, &acc.name);
        }
    }

    // Event discriminators
    let mut evt_discs: HashMap<&[u8], &str> = HashMap::new();
    for evt in &registry.events {
        if let Some(existing) = evt_discs.get(evt.discriminator.as_slice()) {
            sink.error(format!(
                "Discriminator collision: events '{}' and '{}' share discriminator {:?}",
                existing, evt.name, evt.discriminator
            ));
        } else {
            evt_discs.insert(&evt.discriminator, &evt.name);
        }
    }
}
