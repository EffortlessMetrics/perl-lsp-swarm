# Position fixture owner

This private crate owns the independent literal expected-fact corpus for #8172.
Keep raw-byte identity, decode disposition, source subjects, LF records, wire
coordinates, and parser points separate. Do not import production line scanners,
position mappers, or source normalization into its validator. Run its validator
before semantic consumers. ADR-0048 is the accepted LF/BOM authority.
