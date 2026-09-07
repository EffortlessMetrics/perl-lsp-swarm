# DBIx::QuickORM API Return Matrix

Status: generated
Generator: `cargo xtask generate-quickorm-api-matrix`
Check: `cargo xtask generate-quickorm-api-matrix --check`
Authority: `perl-semantic-facts::framework_adapters::quickorm_api` (#13374)

Registry: `quickorm.api-return.1.v1`
Upstream: DBIx::QuickORM `0.000029` at [`99d7d6155933`](https://github.com/exodist/DBIx-QuickORM/tree/99d7d6155933efd54ce7d66d8bfeb9e1ebda81a5)

This matrix pins, for one exact upstream revision, which QuickORM methods return a handle that preserves the receiver's source and row type parameters, which transform them, and which are row terminals, iterators, plain data, metadata, counts, or side effects. It is reviewed reference data consumed by later type propagation; it is not produced by this repository's own inference and it changes no runtime behavior.

Receiver identity and argument cohort are both load-bearing: the same method name means different things on different receivers, and a large family of handle methods return stored state with no arguments but a refined clone with arguments.

| Case | Package | Method | Receiver | Arguments | Return class | Multiplicity | Type params | Mode | Void | Boundary | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `conn.all` | `DBIx::QuickORM::Connection` | `all` | connection | required | multiple_rows | list_of_zero_or_more | derived_from_argument_source | sync_only | permitted | exact | `lib/DBIx/QuickORM/Connection.pm:996` |
| `conn.any` | `DBIx::QuickORM::Connection` | `any` | connection | required | unsupported_dynamic_variant | nothing | not_applicable | not_applicable | permitted | unsupported_at_pinned_version | `lib/DBIx/QuickORM/Connection.pm:998` |
| `conn.aside` | `DBIx::QuickORM::Connection` | `aside` | connection | required | transform_handle_source_row | one | derived_from_argument_source | sync_async_aside_forked | permitted | exact | `lib/DBIx/QuickORM/Connection.pm:993` |
| `conn.async` | `DBIx::QuickORM::Connection` | `async` | connection | required | transform_handle_source_row | one | derived_from_argument_source | sync_async_aside_forked | permitted | exact | `lib/DBIx/QuickORM/Connection.pm:992` |
| `conn.by_id` | `DBIx::QuickORM::Connection` | `by_id` | connection | required | single_optional_row | zero_or_one | derived_from_argument_source | sync_only | permitted | runtime_resolved | `lib/DBIx/QuickORM/Connection.pm:1004` |
| `conn.by_ids` | `DBIx::QuickORM::Connection` | `by_ids` | connection | required | multiple_rows | arrayref_of_zero_or_more | derived_from_argument_source | sync_only | permitted | runtime_resolved | `lib/DBIx/QuickORM/Connection.pm:1043` |
| `conn.count` | `DBIx::QuickORM::Connection` | `count` | connection | required | boolean_or_count | one | not_applicable | sync_only | permitted | exact | `lib/DBIx/QuickORM/Connection.pm:1001` |
| `conn.db` | `DBIx::QuickORM::Connection` | `db` | connection | none | metadata_or_scalar | one | not_applicable | not_applicable | permitted | exact | `lib/DBIx/QuickORM/Connection.pm:433` |
| `conn.delete` | `DBIx::QuickORM::Connection` | `delete` | connection | required | mutation_or_side_effect_result | one | not_applicable | sync_only | permitted | runtime_resolved | `lib/DBIx/QuickORM/Connection.pm:1002` |
| `conn.find_or_insert` | `DBIx::QuickORM::Connection` | `find_or_insert` | connection | trailing_data_hashref | single_optional_row | one | derived_from_argument_source | sync_only | permitted | runtime_resolved | `lib/DBIx/QuickORM/Connection.pm:1011` |
| `conn.first` | `DBIx::QuickORM::Connection` | `first` | connection | required | single_optional_row | zero_or_one | derived_from_argument_source | sync_with_async_row_result | permitted | runtime_resolved | `lib/DBIx/QuickORM/Connection.pm:999` |
| `conn.forked` | `DBIx::QuickORM::Connection` | `forked` | connection | required | transform_handle_source_row | one | derived_from_argument_source | sync_async_aside_forked | permitted | exact | `lib/DBIx/QuickORM/Connection.pm:994` |
| `conn.handle` | `DBIx::QuickORM::Connection` | `handle` | connection | required | transform_handle_source_row | one | derived_from_argument_source | sync_async_aside_forked | permitted | runtime_resolved | `lib/DBIx/QuickORM/Connection.pm:928` |
| `conn.insert` | `DBIx::QuickORM::Connection` | `insert` | connection | trailing_data_hashref | mutation_or_side_effect_result | one | derived_from_argument_source | sync_only | permitted | runtime_resolved | `lib/DBIx/QuickORM/Connection.pm:1006` |
| `conn.iterate` | `DBIx::QuickORM::Connection` | `iterate` | connection | trailing_coderef | mutation_or_side_effect_result | nothing | not_applicable | sync_only | permitted | exact | `lib/DBIx/QuickORM/Connection.pm:1005` |
| `conn.iterator` | `DBIx::QuickORM::Connection` | `iterator` | connection | required | iterator_of_rows | iterator_of_zero_or_more | derived_from_argument_source | sync_async_aside_forked | permitted | runtime_resolved | `lib/DBIx/QuickORM/Connection.pm:997` |
| `conn.one` | `DBIx::QuickORM::Connection` | `one` | connection | required | single_optional_row | zero_or_one | derived_from_argument_source | sync_with_async_row_result | permitted | runtime_resolved | `lib/DBIx/QuickORM/Connection.pm:1000` |
| `conn.source` | `DBIx::QuickORM::Connection` | `source` | connection | required | metadata_or_scalar | zero_or_one | not_applicable | not_applicable | permitted | runtime_resolved | `lib/DBIx/QuickORM/Connection.pm:861` |
| `conn.update` | `DBIx::QuickORM::Connection` | `update` | connection | trailing_data_hashref | mutation_or_side_effect_result | one | not_applicable | sync_only | permitted | runtime_resolved | `lib/DBIx/QuickORM/Connection.pm:1008` |
| `conn.update_or_insert` | `DBIx::QuickORM::Connection` | `update_or_insert` | connection | trailing_data_hashref | mutation_or_side_effect_result | one | derived_from_argument_source | sync_only | permitted | runtime_resolved | `lib/DBIx/QuickORM/Connection.pm:1010` |
| `conn.vivify` | `DBIx::QuickORM::Connection` | `vivify` | connection | trailing_data_hashref | mutation_or_side_effect_result | one | derived_from_argument_source | sync_only | permitted | runtime_resolved | `lib/DBIx/QuickORM/Connection.pm:1007` |
| `dsl.qorm_table` | `DBIx::QuickORM` | `qorm_table` | generated_table_package | none | metadata_or_scalar | one | not_applicable | not_applicable | permitted | exact | `lib/DBIx/QuickORM.pm:951` |
| `handle.all` | `DBIx::QuickORM::Handle` | `all` | handle | optional | multiple_rows | list_of_zero_or_more | preserved_from_receiver | sync_only | permitted | runtime_resolved | `lib/DBIx/QuickORM/Handle.pm:3264` |
| `handle.all_fields` | `DBIx::QuickORM::Handle` | `all_fields` | handle | none | preserve_handle_source_row | one | preserved_from_receiver | sync_async_aside_forked | croaks | exact | `lib/DBIx/QuickORM/Handle.pm:1127` |
| `handle.aside` | `DBIx::QuickORM::Handle` | `aside` | handle | none | preserve_handle_source_row | one | preserved_from_receiver | sync_async_aside_forked | croaks | exact | `lib/DBIx/QuickORM/Handle.pm:1085` |
| `handle.async` | `DBIx::QuickORM::Handle` | `async` | handle | none | preserve_handle_source_row | one | preserved_from_receiver | sync_async_aside_forked | croaks | exact | `lib/DBIx/QuickORM/Handle.pm:1078` |
| `handle.auto_refresh` | `DBIx::QuickORM::Handle` | `auto_refresh` | handle | none | preserve_handle_source_row | one | preserved_from_receiver | sync_async_aside_forked | croaks | exact | `lib/DBIx/QuickORM/Handle.pm:1057` |
| `handle.by_id` | `DBIx::QuickORM::Handle` | `by_id` | handle | required | single_optional_row | zero_or_one | preserved_from_receiver | sync_only | permitted | runtime_resolved | `lib/DBIx/QuickORM/Handle.pm:1898` |
| `handle.by_ids` | `DBIx::QuickORM::Handle` | `by_ids` | handle | required | multiple_rows | arrayref_of_zero_or_more | preserved_from_receiver | sync_only | permitted | runtime_resolved | `lib/DBIx/QuickORM/Handle.pm:1938` |
| `handle.cas` | `DBIx::QuickORM::Handle` | `cas` | handle | required | mutation_or_side_effect_result | one | not_applicable | sync_only | permitted | runtime_resolved | `lib/DBIx/QuickORM/Handle.pm:2729` |
| `handle.clone` | `DBIx::QuickORM::Handle` | `clone` | handle | optional | preserve_handle_source_row | one | preserved_from_receiver | sync_async_aside_forked | permitted | exact | `lib/DBIx/QuickORM/Handle.pm:763` |
| `handle.connection.get` | `DBIx::QuickORM::Handle` | `connection` | handle | zero_arg_getter | metadata_or_scalar | one | not_applicable | sync_async_aside_forked | croaks | exact | `lib/DBIx/QuickORM/Handle.pm:1294` |
| `handle.connection.set` | `DBIx::QuickORM::Handle` | `connection` | handle | value_setter | preserve_handle_source_row | one | preserved_from_receiver | sync_async_aside_forked | croaks | exact | `lib/DBIx/QuickORM/Handle.pm:1294` |
| `handle.count` | `DBIx::QuickORM::Handle` | `count` | handle | optional | boolean_or_count | one | not_applicable | sync_only | permitted | exact | `lib/DBIx/QuickORM/Handle.pm:3313` |
| `handle.cross_join` | `DBIx::QuickORM::Handle` | `cross_join` | handle | required | transform_handle_source_row | one | transformed_to_join_row | sync_async_aside_forked | croaks | runtime_resolved | `lib/DBIx/QuickORM/Handle.pm:391` |
| `handle.data_only` | `DBIx::QuickORM::Handle` | `data_only` | handle | optional | data_only_transition | one | erased_to_plain_data | sync_async_aside_forked | croaks | exact | `lib/DBIx/QuickORM/Handle.pm:1099` |
| `handle.delete` | `DBIx::QuickORM::Handle` | `delete` | handle | optional | mutation_or_side_effect_result | one | not_applicable | sync_only | permitted | runtime_resolved | `lib/DBIx/QuickORM/Handle.pm:2430` |
| `handle.dialect` | `DBIx::QuickORM::Handle` | `dialect` | handle | none | metadata_or_scalar | one | not_applicable | not_applicable | permitted | exact | `lib/DBIx/QuickORM/Handle.pm:202` |
| `handle.fields.get` | `DBIx::QuickORM::Handle` | `fields` | handle | zero_arg_getter | metadata_or_scalar | zero_or_one | not_applicable | sync_async_aside_forked | croaks | exact | `lib/DBIx/QuickORM/Handle.pm:1315` |
| `handle.fields.set` | `DBIx::QuickORM::Handle` | `fields` | handle | value_setter | preserve_handle_source_row | one | preserved_from_receiver | sync_async_aside_forked | croaks | exact | `lib/DBIx/QuickORM/Handle.pm:1315` |
| `handle.first` | `DBIx::QuickORM::Handle` | `first` | handle | optional | single_optional_row | zero_or_one | preserved_from_receiver | sync_with_async_row_result | permitted | runtime_resolved | `lib/DBIx/QuickORM/Handle.pm:3238` |
| `handle.forked` | `DBIx::QuickORM::Handle` | `forked` | handle | none | preserve_handle_source_row | one | preserved_from_receiver | sync_async_aside_forked | croaks | exact | `lib/DBIx/QuickORM/Handle.pm:1092` |
| `handle.full_join` | `DBIx::QuickORM::Handle` | `full_join` | handle | required | transform_handle_source_row | one | transformed_to_join_row | sync_async_aside_forked | croaks | runtime_resolved | `lib/DBIx/QuickORM/Handle.pm:390` |
| `handle.handle` | `DBIx::QuickORM::Handle` | `handle` | handle | optional | transform_handle_source_row | one | derived_from_argument_source | sync_async_aside_forked | permitted | runtime_resolved | `lib/DBIx/QuickORM/Handle.pm:765` |
| `handle.inner_join` | `DBIx::QuickORM::Handle` | `inner_join` | handle | required | transform_handle_source_row | one | transformed_to_join_row | sync_async_aside_forked | croaks | runtime_resolved | `lib/DBIx/QuickORM/Handle.pm:389` |
| `handle.insert` | `DBIx::QuickORM::Handle` | `insert` | handle | optional | mutation_or_side_effect_result | one | preserved_from_receiver | sync_only | permitted | runtime_resolved | `lib/DBIx/QuickORM/Handle.pm:2077` |
| `handle.insert_and_refresh` | `DBIx::QuickORM::Handle` | `insert_and_refresh` | handle | optional | mutation_or_side_effect_result | one | preserved_from_receiver | sync_only | permitted | runtime_resolved | `lib/DBIx/QuickORM/Handle.pm:2083` |
| `handle.internal_transactions` | `DBIx::QuickORM::Handle` | `internal_transactions` | handle | optional | preserve_handle_source_row | one | preserved_from_receiver | sync_async_aside_forked | croaks | exact | `lib/DBIx/QuickORM/Handle.pm:1136` |
| `handle.internal_txns` | `DBIx::QuickORM::Handle` | `internal_txns` | handle | optional | preserve_handle_source_row | one | preserved_from_receiver | sync_async_aside_forked | croaks | exact | `lib/DBIx/QuickORM/Handle.pm:1133` |
| `handle.is_aside` | `DBIx::QuickORM::Handle` | `is_aside` | handle | none | boolean_or_count | one | not_applicable | sync_async_aside_forked | permitted | exact | `lib/DBIx/QuickORM/Handle.pm:1554` |
| `handle.is_async` | `DBIx::QuickORM::Handle` | `is_async` | handle | none | boolean_or_count | one | not_applicable | sync_async_aside_forked | permitted | exact | `lib/DBIx/QuickORM/Handle.pm:1553` |
| `handle.is_forked` | `DBIx::QuickORM::Handle` | `is_forked` | handle | none | boolean_or_count | one | not_applicable | sync_async_aside_forked | permitted | exact | `lib/DBIx/QuickORM/Handle.pm:1555` |
| `handle.is_sync` | `DBIx::QuickORM::Handle` | `is_sync` | handle | none | boolean_or_count | one | not_applicable | sync_async_aside_forked | permitted | exact | `lib/DBIx/QuickORM/Handle.pm:1552` |
| `handle.iterate` | `DBIx::QuickORM::Handle` | `iterate` | handle | trailing_coderef | mutation_or_side_effect_result | nothing | preserved_from_receiver | sync_only | permitted | exact | `lib/DBIx/QuickORM/Handle.pm:3376` |
| `handle.iterator` | `DBIx::QuickORM::Handle` | `iterator` | handle | optional | iterator_of_rows | iterator_of_zero_or_more | preserved_from_receiver | sync_async_aside_forked | permitted | runtime_resolved | `lib/DBIx/QuickORM/Handle.pm:3289` |
| `handle.join` | `DBIx::QuickORM::Handle` | `join` | handle | required | transform_handle_source_row | one | transformed_to_join_row | sync_async_aside_forked | croaks | runtime_resolved | `lib/DBIx/QuickORM/Handle.pm:385` |
| `handle.left_join` | `DBIx::QuickORM::Handle` | `left_join` | handle | required | transform_handle_source_row | one | transformed_to_join_row | sync_async_aside_forked | croaks | runtime_resolved | `lib/DBIx/QuickORM/Handle.pm:387` |
| `handle.limit.get` | `DBIx::QuickORM::Handle` | `limit` | handle | zero_arg_getter | metadata_or_scalar | zero_or_one | not_applicable | sync_async_aside_forked | croaks | exact | `lib/DBIx/QuickORM/Handle.pm:1341` |
| `handle.limit.set` | `DBIx::QuickORM::Handle` | `limit` | handle | value_setter | preserve_handle_source_row | one | preserved_from_receiver | sync_async_aside_forked | croaks | exact | `lib/DBIx/QuickORM/Handle.pm:1341` |
| `handle.new` | `DBIx::QuickORM::Handle` | `new` | handle | optional | transform_handle_source_row | one | derived_from_argument_source | sync_async_aside_forked | permitted | runtime_resolved | `lib/DBIx/QuickORM/Handle.pm:761` |
| `handle.no_auto_refresh` | `DBIx::QuickORM::Handle` | `no_auto_refresh` | handle | none | preserve_handle_source_row | one | preserved_from_receiver | sync_async_aside_forked | croaks | exact | `lib/DBIx/QuickORM/Handle.pm:1064` |
| `handle.no_internal_transactions` | `DBIx::QuickORM::Handle` | `no_internal_transactions` | handle | optional | preserve_handle_source_row | one | preserved_from_receiver | sync_async_aside_forked | croaks | exact | `lib/DBIx/QuickORM/Handle.pm:1143` |
| `handle.no_internal_txns` | `DBIx::QuickORM::Handle` | `no_internal_txns` | handle | optional | preserve_handle_source_row | one | preserved_from_receiver | sync_async_aside_forked | croaks | exact | `lib/DBIx/QuickORM/Handle.pm:1134` |
| `handle.offset.get` | `DBIx::QuickORM::Handle` | `offset` | handle | zero_arg_getter | metadata_or_scalar | zero_or_one | not_applicable | sync_async_aside_forked | croaks | exact | `lib/DBIx/QuickORM/Handle.pm:1348` |
| `handle.offset.set` | `DBIx::QuickORM::Handle` | `offset` | handle | value_setter | preserve_handle_source_row | one | preserved_from_receiver | sync_async_aside_forked | croaks | exact | `lib/DBIx/QuickORM/Handle.pm:1348` |
| `handle.omit.get` | `DBIx::QuickORM::Handle` | `omit` | handle | zero_arg_getter | metadata_or_scalar | zero_or_one | not_applicable | sync_async_aside_forked | croaks | exact | `lib/DBIx/QuickORM/Handle.pm:1328` |
| `handle.omit.set` | `DBIx::QuickORM::Handle` | `omit` | handle | value_setter | preserve_handle_source_row | one | preserved_from_receiver | sync_async_aside_forked | croaks | exact | `lib/DBIx/QuickORM/Handle.pm:1328` |
| `handle.one` | `DBIx::QuickORM::Handle` | `one` | handle | optional | single_optional_row | zero_or_one | preserved_from_receiver | sync_with_async_row_result | permitted | runtime_resolved | `lib/DBIx/QuickORM/Handle.pm:3212` |
| `handle.order_by.get` | `DBIx::QuickORM::Handle` | `order_by` | handle | zero_arg_getter | metadata_or_scalar | zero_or_one | not_applicable | sync_async_aside_forked | croaks | exact | `lib/DBIx/QuickORM/Handle.pm:1369` |
| `handle.order_by.set` | `DBIx::QuickORM::Handle` | `order_by` | handle | value_setter | preserve_handle_source_row | one | preserved_from_receiver | sync_async_aside_forked | croaks | exact | `lib/DBIx/QuickORM/Handle.pm:1369` |
| `handle.right_join` | `DBIx::QuickORM::Handle` | `right_join` | handle | required | transform_handle_source_row | one | transformed_to_join_row | sync_async_aside_forked | croaks | runtime_resolved | `lib/DBIx/QuickORM/Handle.pm:388` |
| `handle.row.get` | `DBIx::QuickORM::Handle` | `row` | handle | zero_arg_getter | metadata_or_scalar | zero_or_one | not_applicable | sync_async_aside_forked | croaks | exact | `lib/DBIx/QuickORM/Handle.pm:1308` |
| `handle.row.set` | `DBIx::QuickORM::Handle` | `row` | handle | value_setter | preserve_handle_source_row | one | preserved_from_receiver | sync_async_aside_forked | croaks | exact | `lib/DBIx/QuickORM/Handle.pm:1308` |
| `handle.source.get` | `DBIx::QuickORM::Handle` | `source` | handle | zero_arg_getter | metadata_or_scalar | one | not_applicable | sync_async_aside_forked | croaks | exact | `lib/DBIx/QuickORM/Handle.pm:1301` |
| `handle.source.set` | `DBIx::QuickORM::Handle` | `source` | handle | value_setter | transform_handle_source_row | one | derived_from_argument_source | sync_async_aside_forked | croaks | runtime_resolved | `lib/DBIx/QuickORM/Handle.pm:1301` |
| `handle.sql_builder.get` | `DBIx::QuickORM::Handle` | `sql_builder` | handle | zero_arg_getter | metadata_or_scalar | one | not_applicable | sync_async_aside_forked | croaks | exact | `lib/DBIx/QuickORM/Handle.pm:1284` |
| `handle.sql_builder.set` | `DBIx::QuickORM::Handle` | `sql_builder` | handle | value_setter | preserve_handle_source_row | one | preserved_from_receiver | sync_async_aside_forked | croaks | exact | `lib/DBIx/QuickORM/Handle.pm:1284` |
| `handle.subquery_alias.get` | `DBIx::QuickORM::Handle` | `subquery_alias` | handle | zero_arg_getter | metadata_or_scalar | zero_or_one | not_applicable | sync_async_aside_forked | croaks | exact | `lib/DBIx/QuickORM/Handle.pm:1440` |
| `handle.subquery_alias.set` | `DBIx::QuickORM::Handle` | `subquery_alias` | handle | value_setter | preserve_handle_source_row | one | preserved_from_receiver | sync_async_aside_forked | croaks | exact | `lib/DBIx/QuickORM/Handle.pm:1440` |
| `handle.sync` | `DBIx::QuickORM::Handle` | `sync` | handle | none | preserve_handle_source_row | one | preserved_from_receiver | sync_async_aside_forked | croaks | exact | `lib/DBIx/QuickORM/Handle.pm:1071` |
| `handle.target.get` | `DBIx::QuickORM::Handle` | `target` | handle | zero_arg_getter | metadata_or_scalar | zero_or_one | not_applicable | sync_async_aside_forked | croaks | exact | `lib/DBIx/QuickORM/Handle.pm:1362` |
| `handle.target.set` | `DBIx::QuickORM::Handle` | `target` | handle | value_setter | preserve_handle_source_row | one | preserved_from_receiver | sync_async_aside_forked | croaks | exact | `lib/DBIx/QuickORM/Handle.pm:1362` |
| `handle.update` | `DBIx::QuickORM::Handle` | `update` | handle | optional | mutation_or_side_effect_result | one | not_applicable | sync_only | permitted | runtime_resolved | `lib/DBIx/QuickORM/Handle.pm:2530` |
| `handle.upsert` | `DBIx::QuickORM::Handle` | `upsert` | handle | optional | mutation_or_side_effect_result | one | preserved_from_receiver | sync_only | permitted | runtime_resolved | `lib/DBIx/QuickORM/Handle.pm:2066` |
| `handle.upsert_and_refresh` | `DBIx::QuickORM::Handle` | `upsert_and_refresh` | handle | optional | mutation_or_side_effect_result | one | preserved_from_receiver | sync_only | permitted | runtime_resolved | `lib/DBIx/QuickORM/Handle.pm:2072` |
| `handle.using_internal_transactions` | `DBIx::QuickORM::Handle` | `using_internal_transactions` | handle | none | boolean_or_count | one | not_applicable | sync_async_aside_forked | permitted | exact | `lib/DBIx/QuickORM/Handle.pm:1556` |
| `handle.vivify` | `DBIx::QuickORM::Handle` | `vivify` | handle | trailing_data_hashref | mutation_or_side_effect_result | one | preserved_from_receiver | sync_only | permitted | runtime_resolved | `lib/DBIx/QuickORM/Handle.pm:1945` |
| `handle.where.get` | `DBIx::QuickORM::Handle` | `where` | handle | zero_arg_getter | metadata_or_scalar | zero_or_one | not_applicable | sync_async_aside_forked | croaks | exact | `lib/DBIx/QuickORM/Handle.pm:1355` |
| `handle.where.set` | `DBIx::QuickORM::Handle` | `where` | handle | value_setter | preserve_handle_source_row | one | preserved_from_receiver | sync_async_aside_forked | croaks | exact | `lib/DBIx/QuickORM/Handle.pm:1355` |
| `handle_source.field_affinity` | `DBIx::QuickORM::Handle` | `field_affinity` | handle_as_derived_source | required | metadata_or_scalar | one | not_applicable | not_applicable | permitted | runtime_resolved | `lib/DBIx/QuickORM/Handle.pm:1502` |
| `handle_source.field_db_name` | `DBIx::QuickORM::Handle` | `field_db_name` | handle_as_derived_source | required | metadata_or_scalar | one | not_applicable | not_applicable | permitted | exact | `lib/DBIx/QuickORM/Handle.pm:1482` |
| `handle_source.field_is_generated` | `DBIx::QuickORM::Handle` | `field_is_generated` | handle_as_derived_source | required | boolean_or_count | one | not_applicable | not_applicable | permitted | exact | `lib/DBIx/QuickORM/Handle.pm:1475` |
| `handle_source.field_orm_name` | `DBIx::QuickORM::Handle` | `field_orm_name` | handle_as_derived_source | required | metadata_or_scalar | one | not_applicable | not_applicable | permitted | exact | `lib/DBIx/QuickORM/Handle.pm:1483` |
| `handle_source.field_type` | `DBIx::QuickORM::Handle` | `field_type` | handle_as_derived_source | required | metadata_or_scalar | zero_or_one | not_applicable | not_applicable | permitted | runtime_resolved | `lib/DBIx/QuickORM/Handle.pm:1494` |
| `handle_source.fields_list_all` | `DBIx::QuickORM::Handle` | `fields_list_all` | handle_as_derived_source | none | metadata_or_scalar | one | not_applicable | not_applicable | permitted | exact | `lib/DBIx/QuickORM/Handle.pm:1477` |
| `handle_source.fields_to_fetch` | `DBIx::QuickORM::Handle` | `fields_to_fetch` | handle_as_derived_source | none | metadata_or_scalar | one | not_applicable | not_applicable | permitted | exact | `lib/DBIx/QuickORM/Handle.pm:1476` |
| `handle_source.fields_to_omit` | `DBIx::QuickORM::Handle` | `fields_to_omit` | handle_as_derived_source | none | metadata_or_scalar | zero_or_one | not_applicable | not_applicable | permitted | exact | `lib/DBIx/QuickORM/Handle.pm:1478` |
| `handle_source.has_field` | `DBIx::QuickORM::Handle` | `has_field` | handle_as_derived_source | required | boolean_or_count | one | not_applicable | not_applicable | permitted | exact | `lib/DBIx/QuickORM/Handle.pm:1485` |
| `handle_source.is_writable` | `DBIx::QuickORM::Handle` | `is_writable` | handle_as_derived_source | none | boolean_or_count | one | not_applicable | not_applicable | permitted | exact | `lib/DBIx/QuickORM/Handle.pm:1470` |
| `handle_source.primary_key` | `DBIx::QuickORM::Handle` | `primary_key` | handle_as_derived_source | none | metadata_or_scalar | zero_or_one | not_applicable | not_applicable | permitted | exact | `lib/DBIx/QuickORM/Handle.pm:1472` |
| `handle_source.row_class` | `DBIx::QuickORM::Handle` | `row_class` | handle_as_derived_source | none | metadata_or_scalar | zero_or_one | not_applicable | not_applicable | permitted | exact | `lib/DBIx/QuickORM/Handle.pm:1473` |
| `handle_source.source_db_moniker` | `DBIx::QuickORM::Handle` | `source_db_moniker` | handle_as_derived_source | none | metadata_or_scalar | one | not_applicable | not_applicable | permitted | runtime_resolved | `lib/DBIx/QuickORM/Handle.pm:1449` |
| `handle_source.source_has_aliases` | `DBIx::QuickORM::Handle` | `source_has_aliases` | handle_as_derived_source | none | boolean_or_count | one | not_applicable | not_applicable | permitted | exact | `lib/DBIx/QuickORM/Handle.pm:1474` |
| `handle_source.source_orm_name` | `DBIx::QuickORM::Handle` | `source_orm_name` | handle_as_derived_source | none | metadata_or_scalar | one | not_applicable | not_applicable | permitted | exact | `lib/DBIx/QuickORM/Handle.pm:1467` |
| `iter.first` | `DBIx::QuickORM::Iterator` | `first` | iterator | none | single_optional_row | zero_or_one | preserved_from_receiver | not_applicable | permitted | runtime_resolved | `lib/DBIx/QuickORM/Iterator.pm:129` |
| `iter.last` | `DBIx::QuickORM::Iterator` | `last` | iterator | none | single_optional_row | zero_or_one | preserved_from_receiver | not_applicable | permitted | runtime_resolved | `lib/DBIx/QuickORM/Iterator.pm:143` |
| `iter.list` | `DBIx::QuickORM::Iterator` | `list` | iterator | none | multiple_rows | list_of_zero_or_more | preserved_from_receiver | not_applicable | permitted | runtime_resolved | `lib/DBIx/QuickORM/Iterator.pm:164` |
| `iter.next` | `DBIx::QuickORM::Iterator` | `next` | iterator | none | single_optional_row | zero_or_one | preserved_from_receiver | not_applicable | permitted | runtime_resolved | `lib/DBIx/QuickORM/Iterator.pm:106` |
| `iter.ready` | `DBIx::QuickORM::Iterator` | `ready` | iterator | none | boolean_or_count | one | not_applicable | not_applicable | permitted | exact | `lib/DBIx/QuickORM/Iterator.pm:184` |
| `orm.connect` | `DBIx::QuickORM::ORM` | `connect` | orm | optional | metadata_or_scalar | one | not_applicable | not_applicable | permitted | runtime_resolved | `lib/DBIx/QuickORM/ORM.pm:143` |
| `orm.connection` | `DBIx::QuickORM::ORM` | `connection` | orm | none | metadata_or_scalar | one | not_applicable | not_applicable | permitted | runtime_resolved | `lib/DBIx/QuickORM/ORM.pm:193` |
| `orm.db` | `DBIx::QuickORM::ORM` | `db` | orm | none | metadata_or_scalar | one | not_applicable | not_applicable | permitted | exact | `lib/DBIx/QuickORM/ORM.pm:122` |
| `orm.disconnect` | `DBIx::QuickORM::ORM` | `disconnect` | orm | none | mutation_or_side_effect_result | nothing | not_applicable | not_applicable | permitted | exact | `lib/DBIx/QuickORM/ORM.pm:180` |
| `orm.handle` | `DBIx::QuickORM::ORM` | `handle` | orm | required | transform_handle_source_row | one | derived_from_argument_source | sync_async_aside_forked | permitted | runtime_resolved | `lib/DBIx/QuickORM/ORM.pm:198` |
| `orm.reconnect` | `DBIx::QuickORM::ORM` | `reconnect` | orm | none | metadata_or_scalar | one | not_applicable | not_applicable | permitted | runtime_resolved | `lib/DBIx/QuickORM/ORM.pm:187` |
| `row.cas` | `DBIx::QuickORM::Row` | `cas` | row | required | mutation_or_side_effect_result | one | not_applicable | sync_only | permitted | runtime_resolved | `lib/DBIx/QuickORM/Row.pm:301` |
| `row.check_sync` | `DBIx::QuickORM::Row` | `check_sync` | row | optional | boolean_or_count | one | not_applicable | sync_only | permitted | runtime_resolved | `lib/DBIx/QuickORM/Row.pm:186` |
| `row.clone` | `DBIx::QuickORM::Row` | `clone` | row | optional | single_optional_row | one | preserved_from_receiver | not_applicable | permitted | runtime_resolved | `lib/DBIx/QuickORM/Row.pm:147` |
| `row.connection` | `DBIx::QuickORM::Row` | `connection` | row | none | metadata_or_scalar | one | not_applicable | not_applicable | permitted | exact | `lib/DBIx/QuickORM/Row.pm:111` |
| `row.delete` | `DBIx::QuickORM::Row` | `delete` | row | none | mutation_or_side_effect_result | one | not_applicable | sync_only | permitted | runtime_resolved | `lib/DBIx/QuickORM/Row.pm:296` |
| `row.desynced_data` | `DBIx::QuickORM::Row` | `desynced_data` | row | none | open_hash_or_hash_sequence | hash | not_applicable | not_applicable | permitted | runtime_resolved | `lib/DBIx/QuickORM/Row.pm:118` |
| `row.discard` | `DBIx::QuickORM::Row` | `discard` | row | none | single_optional_row | one | preserved_from_receiver | not_applicable | permitted | exact | `lib/DBIx/QuickORM/Row.pm:287` |
| `row.field` | `DBIx::QuickORM::Row` | `field` | row | required | metadata_or_scalar | zero_or_one | not_applicable | not_applicable | permitted | runtime_resolved | `lib/DBIx/QuickORM/Row.pm:436` |
| `row.field_is_desynced` | `DBIx::QuickORM::Row` | `field_is_desynced` | row | required | boolean_or_count | one | not_applicable | not_applicable | permitted | runtime_resolved | `lib/DBIx/QuickORM/Row.pm:453` |
| `row.fields` | `DBIx::QuickORM::Row` | `fields` | row | none | open_hash_or_hash_sequence | hash | not_applicable | not_applicable | permitted | runtime_resolved | `lib/DBIx/QuickORM/Row.pm:439` |
| `row.force_sync` | `DBIx::QuickORM::Row` | `force_sync` | row | optional | mutation_or_side_effect_result | one | not_applicable | sync_only | permitted | runtime_resolved | `lib/DBIx/QuickORM/Row.pm:269` |
| `row.has_pending` | `DBIx::QuickORM::Row` | `has_pending` | row | none | boolean_or_count | one | not_applicable | not_applicable | permitted | exact | `lib/DBIx/QuickORM/Row.pm:126` |
| `row.in_storage` | `DBIx::QuickORM::Row` | `in_storage` | row | none | boolean_or_count | one | not_applicable | not_applicable | permitted | exact | `lib/DBIx/QuickORM/Row.pm:123` |
| `row.is_desynced` | `DBIx::QuickORM::Row` | `is_desynced` | row | none | boolean_or_count | one | not_applicable | not_applicable | permitted | exact | `lib/DBIx/QuickORM/Row.pm:125` |
| `row.is_invalid` | `DBIx::QuickORM::Row` | `is_invalid` | row | none | boolean_or_count | one | not_applicable | not_applicable | permitted | exact | `lib/DBIx/QuickORM/Row.pm:120` |
| `row.is_stored` | `DBIx::QuickORM::Row` | `is_stored` | row | none | boolean_or_count | one | not_applicable | not_applicable | permitted | exact | `lib/DBIx/QuickORM/Row.pm:124` |
| `row.is_valid` | `DBIx::QuickORM::Row` | `is_valid` | row | none | boolean_or_count | one | not_applicable | not_applicable | permitted | exact | `lib/DBIx/QuickORM/Row.pm:121` |
| `row.pending_data` | `DBIx::QuickORM::Row` | `pending_data` | row | none | open_hash_or_hash_sequence | hash | not_applicable | not_applicable | permitted | runtime_resolved | `lib/DBIx/QuickORM/Row.pm:117` |
| `row.pending_field` | `DBIx::QuickORM::Row` | `pending_field` | row | required | metadata_or_scalar | zero_or_one | not_applicable | not_applicable | permitted | runtime_resolved | `lib/DBIx/QuickORM/Row.pm:443` |
| `row.pending_fields` | `DBIx::QuickORM::Row` | `pending_fields` | row | none | open_hash_or_hash_sequence | hash | not_applicable | not_applicable | permitted | runtime_resolved | `lib/DBIx/QuickORM/Row.pm:449` |
| `row.raw_field` | `DBIx::QuickORM::Row` | `raw_field` | row | required | metadata_or_scalar | zero_or_one | not_applicable | not_applicable | permitted | runtime_resolved | `lib/DBIx/QuickORM/Row.pm:437` |
| `row.raw_fields` | `DBIx::QuickORM::Row` | `raw_fields` | row | none | open_hash_or_hash_sequence | hash | not_applicable | not_applicable | permitted | runtime_resolved | `lib/DBIx/QuickORM/Row.pm:440` |
| `row.raw_pending_field` | `DBIx::QuickORM::Row` | `raw_pending_field` | row | required | metadata_or_scalar | zero_or_one | not_applicable | not_applicable | permitted | runtime_resolved | `lib/DBIx/QuickORM/Row.pm:446` |
| `row.raw_pending_fields` | `DBIx::QuickORM::Row` | `raw_pending_fields` | row | none | open_hash_or_hash_sequence | hash | not_applicable | not_applicable | permitted | runtime_resolved | `lib/DBIx/QuickORM/Row.pm:451` |
| `row.raw_stored_field` | `DBIx::QuickORM::Row` | `raw_stored_field` | row | required | metadata_or_scalar | zero_or_one | not_applicable | not_applicable | permitted | runtime_resolved | `lib/DBIx/QuickORM/Row.pm:445` |
| `row.raw_stored_fields` | `DBIx::QuickORM::Row` | `raw_stored_fields` | row | none | open_hash_or_hash_sequence | hash | not_applicable | not_applicable | permitted | runtime_resolved | `lib/DBIx/QuickORM/Row.pm:450` |
| `row.refresh` | `DBIx::QuickORM::Row` | `refresh` | row | none | single_optional_row | one | preserved_from_receiver | sync_only | permitted | runtime_resolved | `lib/DBIx/QuickORM/Row.pm:276` |
| `row.row_data` | `DBIx::QuickORM::Row` | `row_data` | row | none | metadata_or_scalar | one | not_applicable | not_applicable | permitted | runtime_resolved | `lib/DBIx/QuickORM/Row.pm:114` |
| `row.row_data_obj` | `DBIx::QuickORM::Row` | `row_data_obj` | row | none | metadata_or_scalar | one | not_applicable | not_applicable | permitted | exact | `lib/DBIx/QuickORM/Row.pm:113` |
| `row.source` | `DBIx::QuickORM::Row` | `source` | row | none | metadata_or_scalar | one | not_applicable | not_applicable | permitted | exact | `lib/DBIx/QuickORM/Row.pm:110` |
| `row.stored_data` | `DBIx::QuickORM::Row` | `stored_data` | row | none | open_hash_or_hash_sequence | hash | not_applicable | not_applicable | permitted | runtime_resolved | `lib/DBIx/QuickORM/Row.pm:116` |
| `row.stored_field` | `DBIx::QuickORM::Row` | `stored_field` | row | required | metadata_or_scalar | zero_or_one | not_applicable | not_applicable | permitted | runtime_resolved | `lib/DBIx/QuickORM/Row.pm:442` |
| `row.stored_fields` | `DBIx::QuickORM::Row` | `stored_fields` | row | none | open_hash_or_hash_sequence | hash | not_applicable | not_applicable | permitted | runtime_resolved | `lib/DBIx/QuickORM/Row.pm:448` |
| `row.track_desync` | `DBIx::QuickORM::Row` | `track_desync` | row | none | boolean_or_count | one | not_applicable | not_applicable | permitted | exact | `lib/DBIx/QuickORM/Row.pm:108` |
| `row.update` | `DBIx::QuickORM::Row` | `update` | row | optional | mutation_or_side_effect_result | one | not_applicable | sync_only | permitted | runtime_resolved | `lib/DBIx/QuickORM/Row.pm:307` |

## Notes

- `conn.all`: Delegates to handle(@_)->all; inherits the handle terminal's sync-only rule.
- `conn.any`: Delegates to handle(@_)->any, but Handle defines no `any` at this commit; classifying it as a row terminal by name would be unfounded.
- `conn.aside`: Builds a handle from the argument source, then marks it aside.
- `conn.async`: Builds a handle from the argument source, then marks it async.
- `conn.by_id`: Trailing argument is the id; delegates to the handle terminal.
- `conn.by_ids`: Returns an arrayref, not a flat list.
- `conn.count`: Delegates to the sync-only handle count.
- `conn.db`: Returns the DB object backing the connection.
- `conn.delete`: Write; result is not a queryable handle.
- `conn.find_or_insert`: one($arg) // insert($arg): the branch taken is runtime state, so the row is produced by either a select or a write.
- `conn.first`: Row terminal on a connection; distinct from Iterator::first.
- `conn.forked`: Builds a handle from the argument source, then marks it forked.
- `conn.handle`: Croaks on undef. A handle passed here is consumed as a derived table, not refined in place.
- `conn.insert`: Write; yields the inserted row through the handle terminal.
- `conn.iterate`: Callback-driven; returns nothing.
- `conn.iterator`: Item identity follows the selected source.
- `conn.one`: Delegates to the handle terminal, which may return undef.
- `conn.source`: Resolves a source; a blessed argument must do Role::Source, and no_fatal turns a miss into undef instead of a croak.
- `conn.update`: Write; result is not a queryable handle.
- `conn.update_or_insert`: Alias onto the handle upsert path; upstream delegation proves equivalence.
- `conn.vivify`: Creates an in-memory row bound to the selected source.
- `dsl.qorm_table`: Installed into a table package as a closure returning a clone of the table definition. It is schema metadata, never a row.
- `handle.all`: Croaks unless sync. Yields a flat list of rows, or of plain data under data_only; item identity follows the handle's source.
- `handle.all_fields`: Clone selecting every field and clearing omit.
- `handle.aside`: Mode clone; returns self when already aside.
- `handle.async`: Mode clone; returns self when already async.
- `handle.auto_refresh`: Config clone; returns self when already set.
- `handle.by_id`: May answer from the row cache, returns raw_fields under data_only, and otherwise falls through to one(), which may be undef.
- `handle.by_ids`: Returns an arrayref of by_id results, not a flat list.
- `handle.cas`: Compare-and-swap write.
- `handle.clone`: Substrate for every refining method.
- `handle.connection.get`: Zero-argument form returns the stored connection.
- `handle.connection.set`: Argument form clones with a new connection.
- `handle.count`: Croaks on an async handle. Returns a count, never a row.
- `handle.cross_join`: Clone whose source is the resulting join; the original row type does not survive.
- `handle.data_only`: Later terminals on this handle yield plain hashes, not blessed rows.
- `handle.delete`: Write; result is not a queryable handle.
- `handle.dialect`: Dialect metadata for the handle's connection.
- `handle.fields.get`: Zero-argument form returns the stored field selection.
- `handle.fields.set`: A single arrayref replaces the selection; other arguments append to it.
- `handle.first`: Row terminal: undef when nothing matches, a plain hash under data_only, or an async row placeholder. Distinct from Iterator::first.
- `handle.forked`: Mode clone; returns self when already forked.
- `handle.full_join`: Clone whose source is the resulting join.
- `handle.handle`: Constructor form; the result's source comes from the arguments.
- `handle.inner_join`: Clone whose source is the resulting join.
- `handle.insert`: Routes through the refreshing path when auto_refresh or a literal write is in play.
- `handle.insert_and_refresh`: Always takes the refreshing path.
- `handle.internal_transactions`: Config clone.
- `handle.internal_txns`: Alias; upstream delegates to internal_transactions, which proves equivalence.
- `handle.is_aside`: Mode predicate.
- `handle.is_async`: Mode predicate.
- `handle.is_forked`: Mode predicate.
- `handle.is_sync`: True only when none of forked, async, or aside is set.
- `handle.iterate`: Croaks unless the final argument is a coderef and the handle is sync; returns nothing.
- `handle.iterator`: Returns an Iterator whose items are rows, or plain data under data_only. Item identity is not erased.
- `handle.join`: Installed as a glob alias onto the shared join implementation.
- `handle.left_join`: Clone whose source is the resulting join.
- `handle.limit.get`: Zero-argument form returns the stored limit.
- `handle.limit.set`: Argument form clones with a new limit.
- `handle.new`: Constructor; the result's source comes from the arguments.
- `handle.no_auto_refresh`: Config clone; returns self when already unset.
- `handle.no_internal_transactions`: Config clone with inverted argument sense.
- `handle.no_internal_txns`: Alias; upstream delegates to no_internal_transactions.
- `handle.offset.get`: Zero-argument form returns the stored offset.
- `handle.offset.set`: Argument form clones with a new offset.
- `handle.omit.get`: Zero-argument form returns the stored omit set.
- `handle.omit.set`: A single arrayref replaces the omit set; other arguments append to it.
- `handle.one`: Returns undef when nothing matches, a plain hash under data_only, or an async row placeholder. It is not unconditionally a row.
- `handle.order_by.get`: Zero-argument form returns the stored ordering.
- `handle.order_by.set`: Several arguments are collected into an arrayref.
- `handle.right_join`: Clone whose source is the resulting join.
- `handle.row.get`: Zero-argument form returns the bound row, if any.
- `handle.row.set`: Binding a row clears the where clause; row and where are mutually exclusive.
- `handle.source.get`: Zero-argument form returns the source object, not a row.
- `handle.source.set`: Replacing the source replaces the handle's row identity.
- `handle.sql_builder.get`: Zero-argument form returns the builder, resolving and caching it if unset.
- `handle.sql_builder.set`: Argument form clones with a new builder.
- `handle.subquery_alias.get`: Zero-argument form returns the stored alias.
- `handle.subquery_alias.set`: Argument form clones with a new alias.
- `handle.sync`: Clears forked, async, and aside together.
- `handle.target.get`: Zero-argument form returns the stored write target.
- `handle.target.set`: Argument form clones with a new write target.
- `handle.update`: Write; result is not a queryable handle.
- `handle.upsert`: Routes through the refreshing path when auto_refresh or a literal write is in play.
- `handle.upsert_and_refresh`: Always takes the refreshing path.
- `handle.using_internal_transactions`: Config predicate.
- `handle.vivify`: Croaks without a trailing data hashref.
- `handle.where.get`: Zero-argument form returns the stored where clause.
- `handle.where.set`: Setting a where clause clears any bound row.
- `handle_source.field_affinity`: Delegates to the inner source, defaulting to string when the field is absent.
- `handle_source.field_db_name`: Identity on a derived table.
- `handle_source.field_is_generated`: Always false for a derived table.
- `handle_source.field_orm_name`: Identity on a derived table.
- `handle_source.field_type`: Undef unless the inner source has the field.
- `handle_source.fields_list_all`: A derived table reports the wildcard rather than an enumerable field list.
- `handle_source.fields_to_fetch`: A derived table reports the wildcard.
- `handle_source.fields_to_omit`: Always undef for a derived table.
- `handle_source.has_field`: A derived table's output columns are not enumerable, so any name is accepted.
- `handle_source.is_writable`: A derived table is read-only.
- `handle_source.primary_key`: Always undef here; a concrete table source answers differently.
- `handle_source.row_class`: Always undef here. Reading this as the generic row-class answer would be wrong: it is the derived-table answer only.
- `handle_source.source_db_moniker`: Renders the inner query as a literal subquery reference with binds.
- `handle_source.source_has_aliases`: Always false for a derived table.
- `handle_source.source_orm_name`: Falls back to the default subquery alias.
- `iter.first`: Resets to the start and returns the first item. Same spelling as the handle and connection terminals, different receiver and different semantics.
- `iter.last`: Exhausts the generator and returns the last item, or undef.
- `iter.list`: Exhausts the generator and returns every item as a flat list.
- `iter.next`: Yields the next item, or empty once exhausted. Item identity is whatever the producing handle put in.
- `iter.ready`: True unless a readiness coderef was supplied and reports otherwise.
- `orm.connect`: Establishes and returns a connection.
- `orm.connection`: Returns the cached connection, connecting on first use.
- `orm.db`: Returns the DB definition.
- `orm.disconnect`: Lifecycle side effect.
- `orm.handle`: Delegates to the connection; the result's source comes from the arguments.
- `orm.reconnect`: Replaces and returns the connection.
- `row.cas`: Delegates to the stored handle's compare-and-swap write.
- `row.check_sync`: Desync check against stored state.
- `row.clone`: Produces another row of the same source identity.
- `row.connection`: Connection behind the row's data object.
- `row.delete`: Delegates to the stored handle's delete.
- `row.desynced_data`: Plain desynced field data, not a blessed row.
- `row.discard`: Clears pending and desync state and returns the same row.
- `row.field`: Inflated single field value. This is the ordinary column accessor; named per-column accessors are an autorow feature and are not modeled here.
- `row.field_is_desynced`: Per-field desync predicate.
- `row.fields`: Inflated field map merged from pending over stored.
- `row.force_sync`: Forces the row back into sync with stored state.
- `row.has_pending`: State predicate.
- `row.in_storage`: State predicate.
- `row.is_desynced`: State predicate.
- `row.is_invalid`: State predicate.
- `row.is_stored`: Alias; upstream delegates to in_storage.
- `row.is_valid`: State predicate.
- `row.pending_data`: Plain pending field data.
- `row.pending_field`: Inflated pending value for one field.
- `row.pending_fields`: Inflated pending field map.
- `row.raw_field`: Uninflated single field value.
- `row.raw_fields`: Uninflated field map; this is what by_id returns under data_only.
- `row.raw_pending_field`: Uninflated pending value for one field.
- `row.raw_pending_fields`: Uninflated pending field map.
- `row.raw_stored_field`: Uninflated stored value for one field.
- `row.raw_stored_fields`: Uninflated stored field map.
- `row.refresh`: Croaks when the row no longer exists, so a successful call yields a row rather than undef.
- `row.row_data`: Active row-data record.
- `row.row_data_obj`: Row-data object itself.
- `row.source`: Source behind the row. Same spelling as the handle accessor, different receiver and no clone behavior.
- `row.stored_data`: Plain stored field data.
- `row.stored_field`: Inflated stored value for one field.
- `row.stored_fields`: Inflated stored field map.
- `row.track_desync`: Constant true on the base row class.
- `row.update`: Write through the stored handle.
