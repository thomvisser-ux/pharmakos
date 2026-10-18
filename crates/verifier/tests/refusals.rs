// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The sim's refusal table and this crate's codes for it, held to each other
//! in both directions (S2's plan, E8).
//!
//! The sim refuses at the seal every construct whose effect needs a later stage
//! (`pharmakos_sim::interpreter::REFUSALS`); this crate refuses each of them by
//! a code of its own ([`pharmakos_verifier::REFUSED_CONSTRUCTS`]), so the editor
//! shows before `submit_plan` what the compile would refuse. A lane that lifts a
//! refusal lifts both rows in one pull request, and this file fails until it
//! does.

use pharmakos_sim::interpreter::REFUSALS;
use pharmakos_verifier::{REFUSED_CONSTRUCTS, code_for_construct};

#[test]
fn every_construct_the_sim_refuses_has_an_e_code() {
    let missing: Vec<&str> = REFUSALS
        .iter()
        .filter(|refusal| code_for_construct(refusal.construct).is_none())
        .map(|refusal| refusal.name)
        .collect();
    assert!(
        missing.is_empty(),
        "the sim refuses these constructs and the verifier has no code for them, so a playbook \
         could pass the editor and be refused at submit: {missing:?}"
    );
}

#[test]
fn every_code_names_a_construct_the_sim_refuses() {
    let stale: Vec<&str> = REFUSED_CONSTRUCTS
        .iter()
        .filter(|(construct, _)| {
            !REFUSALS
                .iter()
                .any(|refusal| refusal.construct == *construct)
        })
        .map(|(construct, _)| *construct)
        .collect();
    assert!(
        stale.is_empty(),
        "the verifier refuses these constructs and the sim no longer does, so the editor would \
         refuse a playbook the sim seals: {stale:?}"
    );
    assert_eq!(
        REFUSED_CONSTRUCTS.len(),
        REFUSALS.len(),
        "one code per refused construct, and one refused construct per code"
    );
}
