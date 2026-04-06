//! picloud-workload
//!
//! Implements the domain traits from picloud-domain.
//! Depends only on picloud-domain — never on other slices.
//! Slices communicate at runtime via the event log.

use picloud_domain::error::Result;

pub mod implementation;
