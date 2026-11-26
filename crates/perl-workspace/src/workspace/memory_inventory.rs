//! Checked inventory of every request-surviving [`WorkspaceIndex`] owner.
#![expect(
    missing_docs,
    reason = "policy:17198-memory-evidence: inventory row names match WorkspaceIndex fields"
)]

/// Classification for one retained owner row in workspace memory evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetainedDisposition {
    PhysicallyRetainedAndMeasured,
    SharedPayloadMeasuredOnce,
    CapacityObservedSeparately,
    OperationalFixedOrBounded,
    TransientNotInRetainedTotal,
    CompatibilityPendingRetirement,
    UnsupportedMeasurementNotProven,
}

/// Static inventory row for one production [`WorkspaceIndex`] owner field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetainedOwnerInventoryRow {
    pub field: &'static str,
    pub disposition: RetainedDisposition,
}

/// Every production field on [`WorkspaceIndex`] that can survive across requests.
///
/// Adding a new retained field without updating this table must fail the
/// inventory completeness test.
pub const WORKSPACE_INDEX_RETAINED_INVENTORY: &[RetainedOwnerInventoryRow] = &[
    RetainedOwnerInventoryRow {
        field: "files",
        disposition: RetainedDisposition::PhysicallyRetainedAndMeasured,
    },
    RetainedOwnerInventoryRow {
        field: "symbols",
        disposition: RetainedDisposition::PhysicallyRetainedAndMeasured,
    },
    RetainedOwnerInventoryRow {
        field: "search_index",
        disposition: RetainedDisposition::PhysicallyRetainedAndMeasured,
    },
    RetainedOwnerInventoryRow {
        field: "global_references",
        disposition: RetainedDisposition::PhysicallyRetainedAndMeasured,
    },
    RetainedOwnerInventoryRow {
        field: "fact_shards",
        disposition: RetainedDisposition::PhysicallyRetainedAndMeasured,
    },
    RetainedOwnerInventoryRow {
        field: "semantic_reference_index",
        disposition: RetainedDisposition::SharedPayloadMeasuredOnce,
    },
    RetainedOwnerInventoryRow {
        field: "semantic_import_export_index",
        disposition: RetainedDisposition::PhysicallyRetainedAndMeasured,
    },
    RetainedOwnerInventoryRow {
        field: "semantic_package_graph_index",
        disposition: RetainedDisposition::PhysicallyRetainedAndMeasured,
    },
    RetainedOwnerInventoryRow {
        field: "document_store",
        disposition: RetainedDisposition::PhysicallyRetainedAndMeasured,
    },
    RetainedOwnerInventoryRow {
        field: "workspace_folders",
        disposition: RetainedDisposition::OperationalFixedOrBounded,
    },
    RetainedOwnerInventoryRow {
        field: "limits",
        disposition: RetainedDisposition::OperationalFixedOrBounded,
    },
    RetainedOwnerInventoryRow {
        field: "resource_limit_rejection",
        disposition: RetainedDisposition::TransientNotInRetainedTotal,
    },
    RetainedOwnerInventoryRow {
        field: "write_version",
        disposition: RetainedDisposition::OperationalFixedOrBounded,
    },
    RetainedOwnerInventoryRow {
        field: "lifecycle_guards",
        disposition: RetainedDisposition::OperationalFixedOrBounded,
    },
];
