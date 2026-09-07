# DBIx::QuickORM API Return Matrix

Status: generated
Generator: `cargo xtask generate-quickorm-api-matrix`
Check: `cargo xtask generate-quickorm-api-matrix --check`
Authority: `perl-semantic-facts::framework_adapters::quickorm_api` (#13374)

Registry: `quickorm.api-return.1.v1`
Upstream: DBIx::QuickORM `0.000029` at [`99d7d6155933`](https://github.com/exodist/DBIx-QuickORM/tree/99d7d6155933efd54ce7d66d8bfeb9e1ebda81a5)

This matrix pins, for one exact upstream revision, which QuickORM methods return a handle that preserves the receiver's source and row type parameters, which transform them, and which are row terminals, iterators, plain data, metadata, counts, or side effects. It is reviewed reference data consumed by later type propagation; it is not produced by this repository's own inference and it changes no runtime behavior.

Receiver identity and argument cohort are both load-bearing: the same method name means different things on different receivers, and a large family of handle methods return stored state with no arguments but a refined clone with arguments.

| Case | Package | Method | Receiver | Arguments | Return class | Multiplicity | Type params | Mode | Void | Boundary | Receiver constraints | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `conn.all` | `DBIx::QuickORM::Connection` | `all` | connection | required | multiple_rows | list_of_zero_or_more | derived_from_argument_source | sync_only | permitted | exact | — | `lib/DBIx/QuickORM/Connection.pm:996` |
| `conn.any` | `DBIx::QuickORM::Connection` | `any` | connection | required | single_optional_row | zero_or_one | derived_from_argument_source | sync_with_async_row_result | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Role/Handle.pm:107` |
| `conn.aside` | `DBIx::QuickORM::Connection` | `aside` | connection | required | transform_handle_source_row | one | derived_from_argument_source | sync_async_aside_forked | croaks | exact | — | `lib/DBIx/QuickORM/Connection.pm:993` |
| `conn.async` | `DBIx::QuickORM::Connection` | `async` | connection | required | transform_handle_source_row | one | derived_from_argument_source | sync_async_aside_forked | croaks | exact | — | `lib/DBIx/QuickORM/Connection.pm:992` |
| `conn.by_id` | `DBIx::QuickORM::Connection` | `by_id` | connection | required | single_optional_row | zero_or_one | derived_from_argument_source | sync_with_async_row_result | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Connection.pm:1004` |
| `conn.by_ids` | `DBIx::QuickORM::Connection` | `by_ids` | connection | required | multiple_rows | arrayref_of_zero_or_more | derived_from_argument_source | conditional_non_sync | permitted | runtime_resolved | `a non-synchronous handle passed in admits at most one id that misses the row cache` | `lib/DBIx/QuickORM/Connection.pm:1043` |
| `conn.count` | `DBIx::QuickORM::Connection` | `count` | connection | required | boolean_or_count | zero_or_one | not_applicable | sync_only | permitted | exact | — | `lib/DBIx/QuickORM/Connection.pm:1001` |
| `conn.db` | `DBIx::QuickORM::Connection` | `db` | connection | none | metadata_or_scalar | one | not_applicable | not_applicable | permitted | exact | — | `lib/DBIx/QuickORM/Connection.pm:433` |
| `conn.delete` | `DBIx::QuickORM::Connection` | `delete` | connection | required | mutation_or_side_effect_result | zero_or_one | not_applicable | sync_async_aside_forked | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Connection.pm:1002` |
| `conn.find_or_insert` | `DBIx::QuickORM::Connection` | `find_or_insert` | connection | trailing_data_hashref | single_optional_row | one | derived_from_argument_source | sync_with_async_row_result | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Connection.pm:1011` |
| `conn.first` | `DBIx::QuickORM::Connection` | `first` | connection | required | single_optional_row | zero_or_one | derived_from_argument_source | sync_with_async_row_result | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Connection.pm:999` |
| `conn.forked` | `DBIx::QuickORM::Connection` | `forked` | connection | required | transform_handle_source_row | one | derived_from_argument_source | sync_async_aside_forked | croaks | exact | — | `lib/DBIx/QuickORM/Connection.pm:994` |
| `conn.handle` | `DBIx::QuickORM::Connection` | `handle` | connection | required | transform_handle_source_row | one | derived_from_argument_source | sync_async_aside_forked | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Connection.pm:928` |
| `conn.insert` | `DBIx::QuickORM::Connection` | `insert` | connection | trailing_data_hashref | single_optional_row | one | derived_from_argument_source | sync_with_async_row_result | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Connection.pm:1006` |
| `conn.iterate` | `DBIx::QuickORM::Connection` | `iterate` | connection | trailing_coderef | mutation_or_side_effect_result | nothing | not_applicable | sync_only | permitted | exact | — | `lib/DBIx/QuickORM/Connection.pm:1005` |
| `conn.iterator` | `DBIx::QuickORM::Connection` | `iterator` | connection | required | iterator_of_rows | iterator_of_zero_or_more | derived_from_argument_source | sync_async_aside_forked | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Connection.pm:997` |
| `conn.one` | `DBIx::QuickORM::Connection` | `one` | connection | required | single_optional_row | zero_or_one | derived_from_argument_source | sync_with_async_row_result | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Connection.pm:1000` |
| `conn.source` | `DBIx::QuickORM::Connection` | `source` | connection | required | metadata_or_scalar | zero_or_one | not_applicable | not_applicable | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Connection.pm:861` |
| `conn.update` | `DBIx::QuickORM::Connection` | `update` | connection | trailing_data_hashref | mutation_or_side_effect_result | zero_or_one | not_applicable | sync_async_aside_forked | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Connection.pm:1008` |
| `conn.update_or_insert` | `DBIx::QuickORM::Connection` | `update_or_insert` | connection | trailing_data_hashref | single_optional_row | one | derived_from_argument_source | sync_with_async_row_result | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Connection.pm:1010` |
| `conn.vivify` | `DBIx::QuickORM::Connection` | `vivify` | connection | trailing_data_hashref | single_optional_row | one | derived_from_argument_source | sync_async_aside_forked | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Connection.pm:1007` |
| `dsl.qorm_table` | `DBIx::QuickORM` | `qorm_table` | generated_table_package | none | metadata_or_scalar | one | not_applicable | not_applicable | permitted | exact | — | `lib/DBIx/QuickORM.pm:951` |
| `handle.all` | `DBIx::QuickORM::Handle` | `all` | handle | optional | multiple_rows | list_of_zero_or_more | preserved_from_receiver | sync_only | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Handle.pm:3264` |
| `handle.all_fields` | `DBIx::QuickORM::Handle` | `all_fields` | handle | none | preserve_handle_source_row | one | preserved_from_receiver | sync_async_aside_forked | croaks | exact | — | `lib/DBIx/QuickORM/Handle.pm:1127` |
| `handle.and` | `DBIx::QuickORM::Handle` | `and` | handle | required | preserve_handle_source_row | one | preserved_from_receiver | sync_async_aside_forked | croaks | exact | — | `lib/DBIx/QuickORM/Handle.pm:1153` |
| `handle.any` | `DBIx::QuickORM::Handle` | `any` | handle | optional | single_optional_row | zero_or_one | preserved_from_receiver | sync_with_async_row_result | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Role/Handle.pm:107` |
| `handle.aside` | `DBIx::QuickORM::Handle` | `aside` | handle | none | preserve_handle_source_row | one | preserved_from_receiver | sync_async_aside_forked | croaks | exact | — | `lib/DBIx/QuickORM/Handle.pm:1085` |
| `handle.async` | `DBIx::QuickORM::Handle` | `async` | handle | none | preserve_handle_source_row | one | preserved_from_receiver | sync_async_aside_forked | croaks | exact | — | `lib/DBIx/QuickORM/Handle.pm:1078` |
| `handle.auto_refresh` | `DBIx::QuickORM::Handle` | `auto_refresh` | handle | none | preserve_handle_source_row | one | preserved_from_receiver | sync_async_aside_forked | croaks | exact | — | `lib/DBIx/QuickORM/Handle.pm:1057` |
| `handle.by_id.copy` | `DBIx::QuickORM::Handle` | `by_id` | handle | no_source_argument | single_optional_row | zero_or_one | preserved_from_receiver | sync_with_async_row_result | permitted | runtime_resolved | `no where clause`; `no associated row`; `source has a primary key` | `lib/DBIx/QuickORM/Handle.pm:1899` |
| `handle.by_id.rebind` | `DBIx::QuickORM::Handle` | `by_id` | handle | source_or_row_rebinding | single_optional_row | zero_or_one | derived_from_argument_source | sync_with_async_row_result | permitted | runtime_resolved | `trailing id argument is always required`; `rebound source has a primary key` | `lib/DBIx/QuickORM/Handle.pm:1900` |
| `handle.by_ids` | `DBIx::QuickORM::Handle` | `by_ids` | handle | required | multiple_rows | arrayref_of_zero_or_more | preserved_from_receiver | conditional_non_sync | permitted | runtime_resolved | `no where clause`; `no associated row`; `a non-synchronous handle admits at most one id that misses the row cache` | `lib/DBIx/QuickORM/Handle.pm:1938` |
| `handle.cas` | `DBIx::QuickORM::Handle` | `cas` | handle | required | mutation_or_side_effect_result | one | not_applicable | sync_async_aside_only | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Handle.pm:2729` |
| `handle.clone.copy` | `DBIx::QuickORM::Handle` | `clone` | handle | no_source_argument | preserve_handle_source_row | one | preserved_from_receiver | sync_async_aside_forked | permitted | exact | — | `lib/DBIx/QuickORM/Handle.pm:763` |
| `handle.clone.rebind` | `DBIx::QuickORM::Handle` | `clone` | handle | source_or_row_rebinding | transform_handle_source_row | one | derived_from_argument_source | sync_async_aside_forked | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Handle.pm:763` |
| `handle.connection.get` | `DBIx::QuickORM::Handle` | `connection` | handle | zero_arg_getter | metadata_or_scalar | one | not_applicable | sync_async_aside_forked | croaks | exact | — | `lib/DBIx/QuickORM/Handle.pm:1294` |
| `handle.connection.set` | `DBIx::QuickORM::Handle` | `connection` | handle | value_setter | preserve_handle_source_row | one | preserved_from_receiver | sync_async_aside_forked | croaks | exact | — | `lib/DBIx/QuickORM/Handle.pm:1294` |
| `handle.count` | `DBIx::QuickORM::Handle` | `count` | handle | optional | boolean_or_count | zero_or_one | not_applicable | sync_only | permitted | exact | — | `lib/DBIx/QuickORM/Handle.pm:3313` |
| `handle.cross_join` | `DBIx::QuickORM::Handle` | `cross_join` | handle | required | transform_handle_source_row | one | transformed_to_join_row | sync_async_aside_forked | croaks | runtime_resolved | — | `lib/DBIx/QuickORM/Handle.pm:391` |
| `handle.data_only.enable` | `DBIx::QuickORM::Handle` | `data_only` | handle | none | data_only_transition | one | erased_to_plain_data | sync_async_aside_forked | croaks | exact | — | `lib/DBIx/QuickORM/Handle.pm:1099` |
| `handle.data_only.set` | `DBIx::QuickORM::Handle` | `data_only` | handle | value_setter | data_only_transition | one | determined_by_argument_value | sync_async_aside_forked | croaks | runtime_resolved | — | `lib/DBIx/QuickORM/Handle.pm:1102` |
| `handle.delete.bound_row` | `DBIx::QuickORM::Handle` | `delete` | handle | optional | mutation_or_side_effect_result | zero_or_one | not_applicable | sync_async_aside_forked | permitted | runtime_resolved | `handle is bound to a row`; `bound row's table has a primary key` | `lib/DBIx/QuickORM/Handle.pm:2505` |
| `handle.delete.bulk` | `DBIx::QuickORM::Handle` | `delete` | handle | optional | mutation_or_side_effect_result | zero_or_one | not_applicable | conditional_non_sync | permitted | runtime_resolved | `no bound row`; `a forked handle is refused whenever no row is bound`; `with a row cache and no `RETURNING` on delete, only a synchronous handle is admitted` | `lib/DBIx/QuickORM/Handle.pm:2510` |
| `handle.dialect` | `DBIx::QuickORM::Handle` | `dialect` | handle | none | metadata_or_scalar | one | not_applicable | not_applicable | permitted | exact | — | `lib/DBIx/QuickORM/Handle.pm:202` |
| `handle.distinct` | `DBIx::QuickORM::Handle` | `distinct` | handle | optional | preserve_handle_source_row | one | preserved_from_receiver | sync_async_aside_forked | croaks | exact | — | `lib/DBIx/QuickORM/Handle.pm:1113` |
| `handle.fields.get` | `DBIx::QuickORM::Handle` | `fields` | handle | zero_arg_getter | metadata_or_scalar | zero_or_one | not_applicable | sync_async_aside_forked | croaks | exact | — | `lib/DBIx/QuickORM/Handle.pm:1315` |
| `handle.fields.set` | `DBIx::QuickORM::Handle` | `fields` | handle | value_setter | preserve_handle_source_row | one | preserved_from_receiver | sync_async_aside_forked | croaks | exact | — | `lib/DBIx/QuickORM/Handle.pm:1315` |
| `handle.first` | `DBIx::QuickORM::Handle` | `first` | handle | optional | single_optional_row | zero_or_one | preserved_from_receiver | sync_with_async_row_result | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Handle.pm:3238` |
| `handle.forked` | `DBIx::QuickORM::Handle` | `forked` | handle | none | preserve_handle_source_row | one | preserved_from_receiver | sync_async_aside_forked | croaks | exact | — | `lib/DBIx/QuickORM/Handle.pm:1092` |
| `handle.full_join` | `DBIx::QuickORM::Handle` | `full_join` | handle | required | transform_handle_source_row | one | transformed_to_join_row | sync_async_aside_forked | croaks | runtime_resolved | — | `lib/DBIx/QuickORM/Handle.pm:390` |
| `handle.handle.copy` | `DBIx::QuickORM::Handle` | `handle` | handle | no_source_argument | preserve_handle_source_row | one | preserved_from_receiver | sync_async_aside_forked | permitted | exact | — | `lib/DBIx/QuickORM/Handle.pm:765` |
| `handle.handle.rebind` | `DBIx::QuickORM::Handle` | `handle` | handle | source_or_row_rebinding | transform_handle_source_row | one | derived_from_argument_source | sync_async_aside_forked | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Handle.pm:765` |
| `handle.inner_join` | `DBIx::QuickORM::Handle` | `inner_join` | handle | required | transform_handle_source_row | one | transformed_to_join_row | sync_async_aside_forked | croaks | runtime_resolved | — | `lib/DBIx/QuickORM/Handle.pm:389` |
| `handle.insert` | `DBIx::QuickORM::Handle` | `insert` | handle | optional | single_optional_row | one | preserved_from_receiver | sync_with_async_row_result | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Handle.pm:2077` |
| `handle.insert_and_refresh` | `DBIx::QuickORM::Handle` | `insert_and_refresh` | handle | optional | single_optional_row | one | preserved_from_receiver | sync_with_async_row_result | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Handle.pm:2083` |
| `handle.internal_transactions` | `DBIx::QuickORM::Handle` | `internal_transactions` | handle | optional | preserve_handle_source_row | one | preserved_from_receiver | sync_async_aside_forked | croaks | exact | — | `lib/DBIx/QuickORM/Handle.pm:1136` |
| `handle.internal_txns` | `DBIx::QuickORM::Handle` | `internal_txns` | handle | optional | preserve_handle_source_row | one | preserved_from_receiver | sync_async_aside_forked | croaks | exact | — | `lib/DBIx/QuickORM/Handle.pm:1133` |
| `handle.is_aside` | `DBIx::QuickORM::Handle` | `is_aside` | handle | none | boolean_or_count | one | not_applicable | sync_async_aside_forked | permitted | exact | — | `lib/DBIx/QuickORM/Handle.pm:1554` |
| `handle.is_async` | `DBIx::QuickORM::Handle` | `is_async` | handle | none | boolean_or_count | one | not_applicable | sync_async_aside_forked | permitted | exact | — | `lib/DBIx/QuickORM/Handle.pm:1553` |
| `handle.is_forked` | `DBIx::QuickORM::Handle` | `is_forked` | handle | none | boolean_or_count | one | not_applicable | sync_async_aside_forked | permitted | exact | — | `lib/DBIx/QuickORM/Handle.pm:1555` |
| `handle.is_sync` | `DBIx::QuickORM::Handle` | `is_sync` | handle | none | boolean_or_count | one | not_applicable | sync_async_aside_forked | permitted | exact | — | `lib/DBIx/QuickORM/Handle.pm:1552` |
| `handle.iterate` | `DBIx::QuickORM::Handle` | `iterate` | handle | trailing_coderef | mutation_or_side_effect_result | nothing | not_applicable | sync_only | permitted | exact | — | `lib/DBIx/QuickORM/Handle.pm:3397` |
| `handle.iterator` | `DBIx::QuickORM::Handle` | `iterator` | handle | optional | iterator_of_rows | iterator_of_zero_or_more | preserved_from_receiver | sync_async_aside_forked | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Handle.pm:3289` |
| `handle.join` | `DBIx::QuickORM::Handle` | `join` | handle | required | transform_handle_source_row | one | transformed_to_join_row | sync_async_aside_forked | croaks | runtime_resolved | — | `lib/DBIx/QuickORM/Handle.pm:385` |
| `handle.left_join` | `DBIx::QuickORM::Handle` | `left_join` | handle | required | transform_handle_source_row | one | transformed_to_join_row | sync_async_aside_forked | croaks | runtime_resolved | — | `lib/DBIx/QuickORM/Handle.pm:387` |
| `handle.limit.get` | `DBIx::QuickORM::Handle` | `limit` | handle | zero_arg_getter | metadata_or_scalar | zero_or_one | not_applicable | sync_async_aside_forked | croaks | exact | — | `lib/DBIx/QuickORM/Handle.pm:1341` |
| `handle.limit.set` | `DBIx::QuickORM::Handle` | `limit` | handle | value_setter | preserve_handle_source_row | one | preserved_from_receiver | sync_async_aside_forked | croaks | exact | — | `lib/DBIx/QuickORM/Handle.pm:1341` |
| `handle.new.copy` | `DBIx::QuickORM::Handle` | `new` | handle | no_source_argument | preserve_handle_source_row | one | preserved_from_receiver | sync_async_aside_forked | permitted | exact | — | `lib/DBIx/QuickORM/Handle.pm:761` |
| `handle.new.rebind` | `DBIx::QuickORM::Handle` | `new` | handle | source_or_row_rebinding | transform_handle_source_row | one | derived_from_argument_source | sync_async_aside_forked | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Handle.pm:761` |
| `handle.no_auto_refresh` | `DBIx::QuickORM::Handle` | `no_auto_refresh` | handle | none | preserve_handle_source_row | one | preserved_from_receiver | sync_async_aside_forked | croaks | exact | — | `lib/DBIx/QuickORM/Handle.pm:1064` |
| `handle.no_internal_transactions` | `DBIx::QuickORM::Handle` | `no_internal_transactions` | handle | optional | preserve_handle_source_row | one | preserved_from_receiver | sync_async_aside_forked | croaks | exact | — | `lib/DBIx/QuickORM/Handle.pm:1143` |
| `handle.no_internal_txns` | `DBIx::QuickORM::Handle` | `no_internal_txns` | handle | optional | preserve_handle_source_row | one | preserved_from_receiver | sync_async_aside_forked | croaks | exact | — | `lib/DBIx/QuickORM/Handle.pm:1134` |
| `handle.offset.get` | `DBIx::QuickORM::Handle` | `offset` | handle | zero_arg_getter | metadata_or_scalar | zero_or_one | not_applicable | sync_async_aside_forked | croaks | exact | — | `lib/DBIx/QuickORM/Handle.pm:1348` |
| `handle.offset.set` | `DBIx::QuickORM::Handle` | `offset` | handle | value_setter | preserve_handle_source_row | one | preserved_from_receiver | sync_async_aside_forked | croaks | exact | — | `lib/DBIx/QuickORM/Handle.pm:1348` |
| `handle.omit.get` | `DBIx::QuickORM::Handle` | `omit` | handle | zero_arg_getter | metadata_or_scalar | zero_or_one | not_applicable | sync_async_aside_forked | croaks | exact | — | `lib/DBIx/QuickORM/Handle.pm:1328` |
| `handle.omit.set` | `DBIx::QuickORM::Handle` | `omit` | handle | value_setter | preserve_handle_source_row | one | preserved_from_receiver | sync_async_aside_forked | croaks | exact | — | `lib/DBIx/QuickORM/Handle.pm:1328` |
| `handle.one` | `DBIx::QuickORM::Handle` | `one` | handle | optional | single_optional_row | zero_or_one | preserved_from_receiver | sync_with_async_row_result | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Handle.pm:3212` |
| `handle.or` | `DBIx::QuickORM::Handle` | `or` | handle | required | preserve_handle_source_row | one | preserved_from_receiver | sync_async_aside_forked | croaks | exact | — | `lib/DBIx/QuickORM/Handle.pm:1159` |
| `handle.order_by.get` | `DBIx::QuickORM::Handle` | `order_by` | handle | zero_arg_getter | metadata_or_scalar | zero_or_one | not_applicable | sync_async_aside_forked | croaks | exact | — | `lib/DBIx/QuickORM/Handle.pm:1369` |
| `handle.order_by.set` | `DBIx::QuickORM::Handle` | `order_by` | handle | value_setter | preserve_handle_source_row | one | preserved_from_receiver | sync_async_aside_forked | croaks | exact | — | `lib/DBIx/QuickORM/Handle.pm:1369` |
| `handle.right_join` | `DBIx::QuickORM::Handle` | `right_join` | handle | required | transform_handle_source_row | one | transformed_to_join_row | sync_async_aside_forked | croaks | runtime_resolved | — | `lib/DBIx/QuickORM/Handle.pm:388` |
| `handle.row.clear` | `DBIx::QuickORM::Handle` | `row` | handle | value_setter | preserve_handle_source_row | one | preserved_from_receiver | sync_async_aside_forked | croaks | exact | — | `lib/DBIx/QuickORM/Handle.pm:908` |
| `handle.row.get` | `DBIx::QuickORM::Handle` | `row` | handle | zero_arg_getter | metadata_or_scalar | zero_or_one | not_applicable | sync_async_aside_forked | croaks | exact | — | `lib/DBIx/QuickORM/Handle.pm:1308` |
| `handle.row.set` | `DBIx::QuickORM::Handle` | `row` | handle | value_setter | preserve_handle_source_row | one | preserved_from_receiver | sync_async_aside_forked | croaks | exact | `argument row must share the receiver's connection`; `argument row's source must share the receiver's `source_orm_name`` | `lib/DBIx/QuickORM/Handle.pm:1312` |
| `handle.source.get` | `DBIx::QuickORM::Handle` | `source` | handle | zero_arg_getter | metadata_or_scalar | one | not_applicable | sync_async_aside_forked | croaks | exact | — | `lib/DBIx/QuickORM/Handle.pm:1301` |
| `handle.source.set` | `DBIx::QuickORM::Handle` | `source` | handle | value_setter | transform_handle_source_row | one | derived_from_argument_source | sync_async_aside_forked | croaks | runtime_resolved | — | `lib/DBIx/QuickORM/Handle.pm:1301` |
| `handle.sql_builder.get` | `DBIx::QuickORM::Handle` | `sql_builder` | handle | zero_arg_getter | metadata_or_scalar | one | not_applicable | sync_async_aside_forked | croaks | exact | — | `lib/DBIx/QuickORM/Handle.pm:1284` |
| `handle.sql_builder.set` | `DBIx::QuickORM::Handle` | `sql_builder` | handle | value_setter | preserve_handle_source_row | one | preserved_from_receiver | sync_async_aside_forked | croaks | exact | — | `lib/DBIx/QuickORM/Handle.pm:1284` |
| `handle.subquery_alias.get` | `DBIx::QuickORM::Handle` | `subquery_alias` | handle | zero_arg_getter | metadata_or_scalar | zero_or_one | not_applicable | sync_async_aside_forked | croaks | exact | — | `lib/DBIx/QuickORM/Handle.pm:1440` |
| `handle.subquery_alias.set` | `DBIx::QuickORM::Handle` | `subquery_alias` | handle | value_setter | preserve_handle_source_row | one | preserved_from_receiver | sync_async_aside_forked | croaks | exact | — | `lib/DBIx/QuickORM/Handle.pm:1440` |
| `handle.sync` | `DBIx::QuickORM::Handle` | `sync` | handle | none | preserve_handle_source_row | one | preserved_from_receiver | sync_async_aside_forked | croaks | exact | — | `lib/DBIx/QuickORM/Handle.pm:1071` |
| `handle.target.get` | `DBIx::QuickORM::Handle` | `target` | handle | zero_arg_getter | metadata_or_scalar | zero_or_one | not_applicable | sync_async_aside_forked | croaks | exact | — | `lib/DBIx/QuickORM/Handle.pm:1362` |
| `handle.target.set` | `DBIx::QuickORM::Handle` | `target` | handle | value_setter | preserve_handle_source_row | one | preserved_from_receiver | sync_async_aside_forked | croaks | exact | — | `lib/DBIx/QuickORM/Handle.pm:1362` |
| `handle.update.bound_row` | `DBIx::QuickORM::Handle` | `update` | handle | optional | mutation_or_side_effect_result | zero_or_one | not_applicable | sync_async_aside_forked | permitted | runtime_resolved | `handle is bound to a row`; `bound row's table has a primary key` | `lib/DBIx/QuickORM/Handle.pm:2700` |
| `handle.update.bulk` | `DBIx::QuickORM::Handle` | `update` | handle | optional | mutation_or_side_effect_result | zero_or_one | not_applicable | conditional_non_sync | permitted | runtime_resolved | `no bound row`; `a non-synchronous handle requires that the connection maintain no row cache` | `lib/DBIx/QuickORM/Handle.pm:2705` |
| `handle.upsert` | `DBIx::QuickORM::Handle` | `upsert` | handle | optional | single_optional_row | one | preserved_from_receiver | sync_with_async_row_result | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Handle.pm:2066` |
| `handle.upsert_and_refresh` | `DBIx::QuickORM::Handle` | `upsert_and_refresh` | handle | optional | single_optional_row | one | preserved_from_receiver | sync_with_async_row_result | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Handle.pm:2072` |
| `handle.using_internal_transactions` | `DBIx::QuickORM::Handle` | `using_internal_transactions` | handle | none | boolean_or_count | one | not_applicable | sync_async_aside_forked | permitted | exact | — | `lib/DBIx/QuickORM/Handle.pm:1556` |
| `handle.vivify.copy` | `DBIx::QuickORM::Handle` | `vivify` | handle | no_source_argument | single_optional_row | one | preserved_from_receiver | sync_async_aside_forked | permitted | runtime_resolved | `trailing data hashref is always required` | `lib/DBIx/QuickORM/Handle.pm:1946` |
| `handle.vivify.rebind` | `DBIx::QuickORM::Handle` | `vivify` | handle | source_or_row_rebinding | single_optional_row | one | derived_from_argument_source | sync_async_aside_forked | permitted | runtime_resolved | `trailing data hashref is always required` | `lib/DBIx/QuickORM/Handle.pm:1950` |
| `handle.where.get` | `DBIx::QuickORM::Handle` | `where` | handle | zero_arg_getter | metadata_or_scalar | zero_or_one | not_applicable | sync_async_aside_forked | croaks | exact | — | `lib/DBIx/QuickORM/Handle.pm:1355` |
| `handle.where.set` | `DBIx::QuickORM::Handle` | `where` | handle | value_setter | preserve_handle_source_row | one | preserved_from_receiver | sync_async_aside_forked | croaks | exact | — | `lib/DBIx/QuickORM/Handle.pm:1355` |
| `handle_source.cachable` | `DBIx::QuickORM::Handle` | `cachable` | handle_as_derived_source | none | boolean_or_count | one | not_applicable | not_applicable | permitted | exact | — | `lib/DBIx/QuickORM/Role/Source.pm:97` |
| `handle_source.field_affinity` | `DBIx::QuickORM::Handle` | `field_affinity` | handle_as_derived_source | required | metadata_or_scalar | one | not_applicable | not_applicable | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Handle.pm:1502` |
| `handle_source.field_db_name` | `DBIx::QuickORM::Handle` | `field_db_name` | handle_as_derived_source | required | metadata_or_scalar | one | not_applicable | not_applicable | permitted | exact | — | `lib/DBIx/QuickORM/Handle.pm:1482` |
| `handle_source.field_is_generated` | `DBIx::QuickORM::Handle` | `field_is_generated` | handle_as_derived_source | required | boolean_or_count | one | not_applicable | not_applicable | permitted | exact | — | `lib/DBIx/QuickORM/Handle.pm:1475` |
| `handle_source.field_orm_name` | `DBIx::QuickORM::Handle` | `field_orm_name` | handle_as_derived_source | required | metadata_or_scalar | one | not_applicable | not_applicable | permitted | exact | — | `lib/DBIx/QuickORM/Handle.pm:1483` |
| `handle_source.field_type` | `DBIx::QuickORM::Handle` | `field_type` | handle_as_derived_source | required | metadata_or_scalar | zero_or_one | not_applicable | not_applicable | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Handle.pm:1494` |
| `handle_source.fields_list_all` | `DBIx::QuickORM::Handle` | `fields_list_all` | handle_as_derived_source | none | metadata_or_scalar | one | not_applicable | not_applicable | permitted | exact | — | `lib/DBIx/QuickORM/Handle.pm:1477` |
| `handle_source.fields_to_fetch` | `DBIx::QuickORM::Handle` | `fields_to_fetch` | handle_as_derived_source | none | metadata_or_scalar | one | not_applicable | not_applicable | permitted | exact | — | `lib/DBIx/QuickORM/Handle.pm:1476` |
| `handle_source.fields_to_omit` | `DBIx::QuickORM::Handle` | `fields_to_omit` | handle_as_derived_source | none | metadata_or_scalar | zero_or_one | not_applicable | not_applicable | permitted | exact | — | `lib/DBIx/QuickORM/Handle.pm:1478` |
| `handle_source.has_field` | `DBIx::QuickORM::Handle` | `has_field` | handle_as_derived_source | required | boolean_or_count | one | not_applicable | not_applicable | permitted | exact | — | `lib/DBIx/QuickORM/Handle.pm:1485` |
| `handle_source.is_writable` | `DBIx::QuickORM::Handle` | `is_writable` | handle_as_derived_source | none | boolean_or_count | one | not_applicable | not_applicable | permitted | exact | — | `lib/DBIx/QuickORM/Handle.pm:1470` |
| `handle_source.primary_key` | `DBIx::QuickORM::Handle` | `primary_key` | handle_as_derived_source | none | metadata_or_scalar | zero_or_one | not_applicable | not_applicable | permitted | exact | — | `lib/DBIx/QuickORM/Handle.pm:1472` |
| `handle_source.row_class` | `DBIx::QuickORM::Handle` | `row_class` | handle_as_derived_source | none | metadata_or_scalar | zero_or_one | not_applicable | not_applicable | permitted | exact | — | `lib/DBIx/QuickORM/Handle.pm:1473` |
| `handle_source.source_db_moniker` | `DBIx::QuickORM::Handle` | `source_db_moniker` | handle_as_derived_source | none | metadata_or_scalar | one | not_applicable | not_applicable | permitted | runtime_resolved | `handle has a source`; `subquery alias is an identifier` | `lib/DBIx/QuickORM/Handle.pm:1449` |
| `handle_source.source_has_aliases` | `DBIx::QuickORM::Handle` | `source_has_aliases` | handle_as_derived_source | none | boolean_or_count | one | not_applicable | not_applicable | permitted | exact | — | `lib/DBIx/QuickORM/Handle.pm:1474` |
| `handle_source.source_orm_name` | `DBIx::QuickORM::Handle` | `source_orm_name` | handle_as_derived_source | none | metadata_or_scalar | one | not_applicable | not_applicable | permitted | exact | — | `lib/DBIx/QuickORM/Handle.pm:1467` |
| `iter.first` | `DBIx::QuickORM::Iterator` | `first` | iterator | none | single_optional_row | zero_or_one | preserved_from_receiver | not_applicable | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Iterator.pm:129` |
| `iter.last` | `DBIx::QuickORM::Iterator` | `last` | iterator | none | single_optional_row | zero_or_one | preserved_from_receiver | not_applicable | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Iterator.pm:143` |
| `iter.list` | `DBIx::QuickORM::Iterator` | `list` | iterator | none | multiple_rows | list_of_zero_or_more | preserved_from_receiver | not_applicable | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Iterator.pm:164` |
| `iter.next` | `DBIx::QuickORM::Iterator` | `next` | iterator | none | single_optional_row | zero_or_one | preserved_from_receiver | not_applicable | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Iterator.pm:106` |
| `iter.ready` | `DBIx::QuickORM::Iterator` | `ready` | iterator | none | boolean_or_count | one | not_applicable | not_applicable | permitted | exact | — | `lib/DBIx/QuickORM/Iterator.pm:184` |
| `orm.connect` | `DBIx::QuickORM::ORM` | `connect` | orm | optional | metadata_or_scalar | one | not_applicable | not_applicable | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/ORM.pm:143` |
| `orm.connection` | `DBIx::QuickORM::ORM` | `connection` | orm | none | metadata_or_scalar | one | not_applicable | not_applicable | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/ORM.pm:193` |
| `orm.db` | `DBIx::QuickORM::ORM` | `db` | orm | optional | metadata_or_scalar | one | not_applicable | not_applicable | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/ORM.pm:122` |
| `orm.disconnect` | `DBIx::QuickORM::ORM` | `disconnect` | orm | none | mutation_or_side_effect_result | nothing | not_applicable | not_applicable | permitted | exact | — | `lib/DBIx/QuickORM/ORM.pm:180` |
| `orm.handle` | `DBIx::QuickORM::ORM` | `handle` | orm | required | transform_handle_source_row | one | derived_from_argument_source | sync_async_aside_forked | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/ORM.pm:198` |
| `orm.reconnect` | `DBIx::QuickORM::ORM` | `reconnect` | orm | none | metadata_or_scalar | one | not_applicable | not_applicable | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/ORM.pm:187` |
| `row.cas` | `DBIx::QuickORM::Row` | `cas` | row | required | mutation_or_side_effect_result | one | not_applicable | not_applicable | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Row.pm:301` |
| `row.check_pk` | `DBIx::QuickORM::Row` | `check_pk` | row | none | single_optional_row | one | preserved_from_receiver | not_applicable | permitted | exact | — | `lib/DBIx/QuickORM/Role/Row.pm:183` |
| `row.check_sync` | `DBIx::QuickORM::Row` | `check_sync` | row | optional | single_optional_row | one | preserved_from_receiver | not_applicable | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Row.pm:186` |
| `row.clone` | `DBIx::QuickORM::Row` | `clone` | row | optional | single_optional_row | one | preserved_from_receiver | not_applicable | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Row.pm:147` |
| `row.conflate_args` | `DBIx::QuickORM::Row` | `conflate_args` | row | required | open_hash_or_hash_sequence | key_value_sequence | not_applicable | not_applicable | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Role/Row.pm:156` |
| `row.connection` | `DBIx::QuickORM::Row` | `connection` | row | none | metadata_or_scalar | one | not_applicable | not_applicable | permitted | exact | — | `lib/DBIx/QuickORM/Row.pm:111` |
| `row.delete` | `DBIx::QuickORM::Row` | `delete` | row | none | mutation_or_side_effect_result | nothing | not_applicable | not_applicable | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Row.pm:296` |
| `row.desynced_data` | `DBIx::QuickORM::Row` | `desynced_data` | row | none | open_hash_or_hash_sequence | optional_hash | not_applicable | not_applicable | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Row.pm:118` |
| `row.dialect` | `DBIx::QuickORM::Row` | `dialect` | row | none | metadata_or_scalar | one | not_applicable | not_applicable | permitted | exact | — | `lib/DBIx/QuickORM/Role/Row.pm:97` |
| `row.discard` | `DBIx::QuickORM::Row` | `discard` | row | none | single_optional_row | one | preserved_from_receiver | not_applicable | permitted | exact | — | `lib/DBIx/QuickORM/Row.pm:287` |
| `row.display` | `DBIx::QuickORM::Row` | `display` | row | none | metadata_or_scalar | one | not_applicable | not_applicable | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Role/Row.pm:138` |
| `row.field` | `DBIx::QuickORM::Row` | `field` | row | required | metadata_or_scalar | zero_or_one | not_applicable | not_applicable | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Row.pm:436` |
| `row.field_affinity` | `DBIx::QuickORM::Row` | `field_affinity` | row | required | metadata_or_scalar | one | not_applicable | not_applicable | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Role/Row.pm:100` |
| `row.field_is_desynced` | `DBIx::QuickORM::Row` | `field_is_desynced` | row | required | boolean_or_count | one | not_applicable | not_applicable | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Row.pm:453` |
| `row.fields` | `DBIx::QuickORM::Row` | `fields` | row | none | open_hash_or_hash_sequence | hash | not_applicable | not_applicable | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Row.pm:439` |
| `row.follow` | `DBIx::QuickORM::Row` | `follow` | row | required | transform_handle_source_row | one | derived_from_argument_source | not_applicable | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Role/Row.pm:333` |
| `row.force_sync` | `DBIx::QuickORM::Row` | `force_sync` | row | optional | single_optional_row | one | preserved_from_receiver | not_applicable | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Row.pm:269` |
| `row.handle.copy` | `DBIx::QuickORM::Row` | `handle` | row | no_source_argument | transform_handle_source_row | one | preserved_from_receiver | not_applicable | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Role/Row.pm:121` |
| `row.handle.rebind` | `DBIx::QuickORM::Row` | `handle` | row | source_or_row_rebinding | transform_handle_source_row | one | derived_from_argument_source | not_applicable | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Role/Row.pm:121` |
| `row.has_field` | `DBIx::QuickORM::Row` | `has_field` | row | required | boolean_or_count | one | not_applicable | not_applicable | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Role/Row.pm:99` |
| `row.has_pending` | `DBIx::QuickORM::Row` | `has_pending` | row | none | boolean_or_count | one | not_applicable | not_applicable | permitted | exact | — | `lib/DBIx/QuickORM/Row.pm:126` |
| `row.in_storage` | `DBIx::QuickORM::Row` | `in_storage` | row | none | boolean_or_count | one | not_applicable | not_applicable | permitted | exact | — | `lib/DBIx/QuickORM/Row.pm:123` |
| `row.insert` | `DBIx::QuickORM::Row` | `insert` | row | optional | single_optional_row | one | preserved_from_receiver | not_applicable | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Role/Row.pm:239` |
| `row.insert_or_save` | `DBIx::QuickORM::Row` | `insert_or_save` | row | optional | single_optional_row | one | preserved_from_receiver | not_applicable | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Role/Row.pm:230` |
| `row.insert_related` | `DBIx::QuickORM::Row` | `insert_related` | row | required | single_optional_row | one | derived_from_argument_source | not_applicable | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Role/Row.pm:358` |
| `row.is_desynced` | `DBIx::QuickORM::Row` | `is_desynced` | row | none | boolean_or_count | one | not_applicable | not_applicable | permitted | exact | — | `lib/DBIx/QuickORM/Row.pm:125` |
| `row.is_invalid` | `DBIx::QuickORM::Row` | `is_invalid` | row | none | boolean_or_count | one | not_applicable | not_applicable | permitted | exact | — | `lib/DBIx/QuickORM/Row.pm:120` |
| `row.is_stored` | `DBIx::QuickORM::Row` | `is_stored` | row | none | boolean_or_count | one | not_applicable | not_applicable | permitted | exact | — | `lib/DBIx/QuickORM/Row.pm:124` |
| `row.is_valid` | `DBIx::QuickORM::Row` | `is_valid` | row | none | boolean_or_count | one | not_applicable | not_applicable | permitted | exact | — | `lib/DBIx/QuickORM/Row.pm:121` |
| `row.obtain` | `DBIx::QuickORM::Row` | `obtain` | row | required | single_optional_row | zero_or_one | derived_from_argument_source | not_applicable | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Role/Row.pm:348` |
| `row.pending_data` | `DBIx::QuickORM::Row` | `pending_data` | row | none | open_hash_or_hash_sequence | optional_hash | not_applicable | not_applicable | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Row.pm:117` |
| `row.pending_field` | `DBIx::QuickORM::Row` | `pending_field` | row | required | metadata_or_scalar | zero_or_one | not_applicable | not_applicable | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Row.pm:443` |
| `row.pending_fields` | `DBIx::QuickORM::Row` | `pending_fields` | row | none | open_hash_or_hash_sequence | hash | not_applicable | not_applicable | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Row.pm:449` |
| `row.primary_key_field_list` | `DBIx::QuickORM::Row` | `primary_key_field_list` | row | none | metadata_or_scalar | list_of_zero_or_more | not_applicable | not_applicable | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Role/Row.pm:103` |
| `row.primary_key_hash` | `DBIx::QuickORM::Row` | `primary_key_hash` | row | none | open_hash_or_hash_sequence | key_value_sequence | not_applicable | not_applicable | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Role/Row.pm:105` |
| `row.primary_key_hashref` | `DBIx::QuickORM::Row` | `primary_key_hashref` | row | none | open_hash_or_hash_sequence | hash | not_applicable | not_applicable | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Role/Row.pm:106` |
| `row.primary_key_value_list` | `DBIx::QuickORM::Row` | `primary_key_value_list` | row | none | metadata_or_scalar | list_of_zero_or_more | not_applicable | not_applicable | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Role/Row.pm:104` |
| `row.raw_field` | `DBIx::QuickORM::Row` | `raw_field` | row | required | metadata_or_scalar | zero_or_one | not_applicable | not_applicable | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Row.pm:437` |
| `row.raw_fields` | `DBIx::QuickORM::Row` | `raw_fields` | row | none | open_hash_or_hash_sequence | hash | not_applicable | not_applicable | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Row.pm:440` |
| `row.raw_pending_field` | `DBIx::QuickORM::Row` | `raw_pending_field` | row | required | metadata_or_scalar | zero_or_one | not_applicable | not_applicable | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Row.pm:446` |
| `row.raw_pending_fields` | `DBIx::QuickORM::Row` | `raw_pending_fields` | row | none | open_hash_or_hash_sequence | hash | not_applicable | not_applicable | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Row.pm:451` |
| `row.raw_stored_field` | `DBIx::QuickORM::Row` | `raw_stored_field` | row | required | metadata_or_scalar | zero_or_one | not_applicable | not_applicable | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Row.pm:445` |
| `row.raw_stored_fields` | `DBIx::QuickORM::Row` | `raw_stored_fields` | row | none | open_hash_or_hash_sequence | hash | not_applicable | not_applicable | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Row.pm:450` |
| `row.refresh` | `DBIx::QuickORM::Row` | `refresh` | row | none | single_optional_row | one | preserved_from_receiver | not_applicable | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Row.pm:276` |
| `row.row_data` | `DBIx::QuickORM::Row` | `row_data` | row | none | metadata_or_scalar | one | not_applicable | not_applicable | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Row.pm:114` |
| `row.row_data_obj` | `DBIx::QuickORM::Row` | `row_data_obj` | row | none | metadata_or_scalar | one | not_applicable | not_applicable | permitted | exact | — | `lib/DBIx/QuickORM/Row.pm:113` |
| `row.save` | `DBIx::QuickORM::Row` | `save` | row | optional | single_optional_row | one | preserved_from_receiver | not_applicable | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Role/Row.pm:250` |
| `row.siblings` | `DBIx::QuickORM::Row` | `siblings` | row | required | transform_handle_source_row | one | preserved_from_receiver | not_applicable | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Role/Row.pm:373` |
| `row.source` | `DBIx::QuickORM::Row` | `source` | row | none | metadata_or_scalar | one | not_applicable | not_applicable | permitted | exact | — | `lib/DBIx/QuickORM/Row.pm:110` |
| `row.stored_data` | `DBIx::QuickORM::Row` | `stored_data` | row | none | open_hash_or_hash_sequence | optional_hash | not_applicable | not_applicable | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Row.pm:116` |
| `row.stored_field` | `DBIx::QuickORM::Row` | `stored_field` | row | required | metadata_or_scalar | zero_or_one | not_applicable | not_applicable | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Row.pm:442` |
| `row.stored_fields` | `DBIx::QuickORM::Row` | `stored_fields` | row | none | open_hash_or_hash_sequence | hash | not_applicable | not_applicable | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Row.pm:448` |
| `row.track_desync` | `DBIx::QuickORM::Row` | `track_desync` | row | none | boolean_or_count | one | not_applicable | not_applicable | permitted | exact | — | `lib/DBIx/QuickORM/Row.pm:108` |
| `row.update` | `DBIx::QuickORM::Row` | `update` | row | optional | single_optional_row | one | preserved_from_receiver | not_applicable | permitted | runtime_resolved | — | `lib/DBIx/QuickORM/Row.pm:307` |

## Notes

- `conn.all`: Delegates to handle(@_)->all; inherits the handle terminal's sync-only rule.
- `conn.any`: Delegates to the handle's `any`, which `Role::Handle` (composed at Handle.pm:28) defines as `shift->first(@_)`. It is an alias for first, not an unsupported method.
- `conn.aside`: Builds a handle from the argument source, then marks it aside. Implemented as a tail call `$self->handle(@_)->aside` (Connection.pm:992-994), so Perl propagates the caller's context and the handle refiner's `defined wantarray` guard croaks in void context.
- `conn.async`: Builds a handle from the argument source, then marks it async. Implemented as a tail call `$self->handle(@_)->async` (Connection.pm:992-994), so Perl propagates the caller's context and the handle refiner's `defined wantarray` guard croaks in void context.
- `conn.by_id`: Trailing argument is the id; delegates to the handle terminal.
- `conn.by_ids`: Returns an arrayref, not a flat list. Delegates to the handle terminal and inherits its conditional mode: a connection builds a synchronous handle, so the plain call is safe, but a non-synchronous handle supplied through the arguments carries `handle.by_ids`'s at-most-one-cache-miss limit.
- `conn.count`: Delegates to the sync-only handle count, which may be undef.
- `conn.db`: Returns the DB object backing the connection.
- `conn.delete`: Delegates to the handle write; undef when synchronous.
- `conn.find_or_insert`: one($arg) // insert($arg): the branch taken is runtime state, so the row comes from either a select or a write.
- `conn.first`: Row terminal on a connection; distinct from Iterator::first.
- `conn.forked`: Builds a handle from the argument source, then marks it forked. Implemented as a tail call `$self->handle(@_)->forked` (Connection.pm:992-994), so Perl propagates the caller's context and the handle refiner's `defined wantarray` guard croaks in void context.
- `conn.handle`: Croaks on undef. A handle passed here is consumed as a derived table, not refined in place.
- `conn.insert`: Delegates to the handle write, which returns the inserted row.
- `conn.iterate`: Callback-driven; returns nothing.
- `conn.iterator`: Item identity follows the selected source.
- `conn.one`: Delegates to the handle terminal, which may return undef.
- `conn.source`: Resolves a source; a blessed argument must do Role::Source, and no_fatal turns a miss into undef instead of a croak.
- `conn.update`: Delegates to the handle write; undef when synchronous.
- `conn.update_or_insert`: Alias onto the handle upsert path; upstream delegation proves equivalence. Returns the row.
- `conn.vivify`: Delegates to handle vivify, returning an in-memory row bound to the selected source.
- `dsl.qorm_table`: Installed into a table package as a closure returning a clone of the table definition. It is schema metadata, never a row.
- `handle.all`: Croaks unless sync. Yields a flat list of rows, or of plain data under data_only; item identity follows the handle's source.
- `handle.all_fields`: Clone selecting every field and clearing omit.
- `handle.and`: Installed as a named closure in a glob block (Handle.pm:1151-1163). Clones with the existing where combined through `qorm_and`; source and row are untouched.
- `handle.any`: Supplied by `Role::Handle`, composed at Handle.pm:28, as a plain alias for first. Not defined in Handle.pm itself.
- `handle.aside`: Mode clone; returns self when already aside.
- `handle.async`: Mode clone; returns self when already async.
- `handle.auto_refresh`: Config clone; returns self when already set.
- `handle.by_id.copy`: Called with only the trailing id, `shift->handle()` receives an empty list and copies the receiver, so the row keeps the receiver's source. May answer from the row cache, returns raw_fields under data_only, and otherwise falls through to one(), which may be undef. There is no synchronous-handle gate.
- `handle.by_id.rebind`: `my $id = pop; my $self = shift->handle(@_)` forwards every leading argument into `Handle::handle`, which can replace SOURCE. The source, primary key and cache lookup then all read the rebound handle (Handle.pm:1905-1906,1933), so the row comes from the rebound source, not the receiver's. The where/row croaks are likewise checked against the rebound handle (Handle.pm:1902-1903).
- `handle.by_ids`: Returns an arrayref of by_id results, not a flat list. Upstream maps sequential `by_id` calls (Handle.pm:1943), and each cache miss reaches `one()` -> `_do_select` -> `_make_sth`, which calls `pid_and_async_check` (Handle.pm:1615). On a non-synchronous handle the first miss leaves an async query in flight, so the second miss confesses `There is currently an async query running` (Connection.pm:398). Ids answered from the row cache issue no query, so a call whose misses number zero or one still succeeds — the failure depends on cache state, not on the id count alone.
- `handle.cas`: Returns a CAS::Result (Handle.pm:2788), not a row. Croaks on a forked handle (Handle.pm:2741); an async or aside result resolves lazily.
- `handle.clone.copy`: Upstream clone, new, and handle are one code path: clone is `$self->handle(@_)` (Handle.pm:763) and new is `$proto->handle(@_)` (Handle.pm:761). With no source or row argument the result is a preserving copy.
- `handle.clone.rebind`: Upstream clone, new, and handle are one code path: clone is `$self->handle(@_)` (Handle.pm:763) and new is `$proto->handle(@_)` (Handle.pm:761). An argument doing Role::Source or Role::Row overwrites SOURCE or ROW on the clone (Handle.pm:797-843), so the receiver's row type does not survive.
- `handle.connection.get`: Zero-argument form returns the stored connection.
- `handle.connection.set`: Argument form clones with a new connection.
- `handle.count`: Croaks on an async handle. Returns a count, never a row, and undef when the select yields no row at all (Handle.pm:3324) — reachable when an inherited offset suppresses the aggregate row.
- `handle.cross_join`: Clone whose source is the resulting join; the original row type does not survive.
- `handle.data_only.enable`: The no-argument form clones with data-only set, so later terminals yield plain hashes instead of blessed rows.
- `handle.data_only.set`: A truthy value erases row identity; `data_only(0)` clones with the mode cleared and restores blessed-row terminals (Handle.pm:1102-1105). The effect follows the argument's truthiness, so a consumer must read the value, not just the call.
- `handle.delete.bound_row`: With a bound row every mode is admitted, cache maintenance running as `on_finish` for a forked delete and `on_ready` otherwise (Handle.pm:2503-2507). Returns the statement handle on a non-sync handle and undef when synchronous (Handle.pm:2525-2528); never a row.
- `handle.delete.bulk`: Three-way conditional. With no row cache the delete runs in any mode and returns the statement handle unless synchronous (Handle.pm:2460-2464). With a cache and a dialect supporting `RETURNING` on delete, async and aside run but forked croaks outright (Handle.pm:2500-2501). With a cache and no `RETURNING`, upstream croaks unless synchronous, because the deleted rows must be identified by a synchronous SELECT-then-DELETE in an internal transaction (Handle.pm:2510). Both the cache and the dialect capability are runtime properties.
- `handle.dialect`: Dialect metadata for the handle's connection.
- `handle.distinct`: Config clone; returns self when already distinct. A falsy argument clears the flag without changing row identity.
- `handle.fields.get`: Zero-argument form returns the stored field selection.
- `handle.fields.set`: A single arrayref replaces the selection; other arguments append to it.
- `handle.first`: Row terminal: undef when nothing matches, a plain hash under data_only, or an async row placeholder. Distinct from Iterator::first.
- `handle.forked`: Mode clone; returns self when already forked.
- `handle.full_join`: Clone whose source is the resulting join.
- `handle.handle.copy`: With no source or row argument this is the same preserving copy as clone.
- `handle.handle.rebind`: An argument doing Role::Source or Role::Row replaces the source or row (Handle.pm:797-843).
- `handle.inner_join`: Clone whose source is the resulting join.
- `handle.insert`: Returns the inserted row: `_insert` ends in `state_insert_row` (Handle.pm:2312-2323), or a `Row::Async` placeholder on an async statement (Handle.pm:2305). Routes through the refreshing path when auto_refresh or a literal write is in play.
- `handle.insert_and_refresh`: Always takes the refreshing path, which branches on `is_sync` (Handle.pm:2141-2147) rather than refusing a non-sync handle.
- `handle.internal_transactions`: Config clone.
- `handle.internal_txns`: Alias; upstream delegates to internal_transactions, which proves equivalence.
- `handle.is_aside`: Mode predicate.
- `handle.is_async`: Mode predicate.
- `handle.is_forked`: Mode predicate.
- `handle.is_sync`: True only when none of forked, async, or aside is set.
- `handle.iterate`: Croaks unless the final argument is a coderef and the handle is sync, then ends in a bare `return` (Handle.pm:3397). Because it yields nothing, there is no returned value to carry a type parameter. The rows the callback receives are built from the retained source (Handle.pm:3388-3392), but that is the callback's argument contract, not this call's return, and it is deliberately not recorded here.
- `handle.iterator`: Returns an Iterator whose items are rows, or plain data under data_only. Item identity is not erased.
- `handle.join`: Installed as a glob alias onto the shared join implementation.
- `handle.left_join`: Clone whose source is the resulting join.
- `handle.limit.get`: Zero-argument form returns the stored limit.
- `handle.limit.set`: Argument form clones with a new limit.
- `handle.new.copy`: Upstream documents new, handle and clone as interchangeable aliases usable on an existing instance or on the class (Handle.pm:486-490); new is `$proto->handle(@_)`. Called on an existing handle with no source or row argument it is a preserving copy, exactly like clone.
- `handle.new.rebind`: Upstream documents new, handle and clone as interchangeable aliases usable on an existing instance or on the class (Handle.pm:486-490); new is `$proto->handle(@_)`. Called on the class, or with a source or row argument, the result's source comes from the arguments.
- `handle.no_auto_refresh`: Config clone; returns self when already unset.
- `handle.no_internal_transactions`: Config clone with inverted argument sense.
- `handle.no_internal_txns`: Alias; upstream delegates to no_internal_transactions.
- `handle.offset.get`: Zero-argument form returns the stored offset.
- `handle.offset.set`: Argument form clones with a new offset.
- `handle.omit.get`: Zero-argument form returns the stored omit set.
- `handle.omit.set`: A single arrayref replaces the omit set; other arguments append to it.
- `handle.one`: Returns undef when nothing matches, a plain hash under data_only, or an async row placeholder. It is not unconditionally a row.
- `handle.or`: Installed as a named closure in a glob block (Handle.pm:1151-1163). Clones with the existing where combined through `qorm_or`; source and row are untouched.
- `handle.order_by.get`: Zero-argument form returns the stored ordering.
- `handle.order_by.set`: Several arguments are collected into an arrayref.
- `handle.right_join`: Clone whose source is the resulting join.
- `handle.row.clear`: `row(undef)` reaches the named-key path with an undefined value, which deletes the ROW slot and returns (Handle.pm:908-912). The source is untouched, so the clone is an unbound handle over the same source: clearing the binding does not mean later terminals have no row type, only that it is derived from the retained source rather than from a bound row.
- `handle.row.get`: Zero-argument form returns the bound row, if any.
- `handle.row.set`: Binding a row clears the where clause but keeps the receiver's source. `row($r)` calls `clone(ROW() => $r, WHERE() => undef)` (Handle.pm:1312), so the row travels as a *named key* and takes the constant path at Handle.pm:902-928, which assigns the slot and never touches SOURCE. The `if ($set{+SOURCE})` branch that can adopt a row's own source is the *positional* `Role::Row` argument form (Handle.pm:829-840), which this call never reaches. `_check_row` admits any row on a matching connection whose source shares a `source_orm_name` (Handle.pm:291-304) and does not require a matching row class, so the source survives while the bound row's class need not.
- `handle.source.get`: Zero-argument form returns the source object, not a row.
- `handle.source.set`: Replacing the source replaces the handle's row identity.
- `handle.sql_builder.get`: Zero-argument form returns the builder, resolving and caching it if unset.
- `handle.sql_builder.set`: Argument form clones with a new builder.
- `handle.subquery_alias.get`: Zero-argument form returns the stored alias.
- `handle.subquery_alias.set`: Argument form clones with a new alias.
- `handle.sync`: Clears forked, async, and aside together.
- `handle.target.get`: Zero-argument form returns the stored write target.
- `handle.target.set`: Argument form clones with a new write target.
- `handle.update.bound_row`: With a bound row every mode is admitted: cache maintenance runs as `on_finish` for a forked write and `on_ready` for sync/async (Handle.pm:2698-2702). Returns the statement handle on a non-sync handle and undef when synchronous (Handle.pm:2718-2728); never a row. Croaks when the bound row's table has no primary key, since no WHERE could be derived (Handle.pm:2551-2552).
- `handle.update.bulk`: Without a bound row the admitted modes depend on the connection. With no row cache the write runs in any mode and returns the statement handle unless synchronous (Handle.pm:2660-2668). With a cache active, upstream croaks unless the handle is synchronous, because identifying the updated rows needs a synchronous SELECT-then-UPDATE in an internal transaction (Handle.pm:2705). `state_does_cache` is a runtime property, so this cannot be resolved statically.
- `handle.upsert`: Same row-returning path as insert, with the upsert flag set (Handle.pm:2066-2070).
- `handle.upsert_and_refresh`: Always takes the refreshing path; returns the row.
- `handle.using_internal_transactions`: Config predicate.
- `handle.vivify.copy`: Croaks with fewer than two arguments and without a trailing data hashref (Handle.pm:1946-1948). Called with only that hashref, `shift->handle()` receives an empty list and copies the receiver, so the vivified row keeps the receiver's source. There is no synchronous-handle gate.
- `handle.vivify.rebind`: `my $data = pop; ... my $self = shift->handle(@_)` forwards every leading argument into `Handle::handle`, which can replace SOURCE. `state_vivify_row` is then passed `source => $self->{+SOURCE}` (Handle.pm:1954-1955), so the in-memory row is bound to the rebound source, not the receiver's.
- `handle.where.get`: Zero-argument form returns the stored where clause.
- `handle.where.set`: Setting a where clause clears any bound row.
- `handle_source.cachable`: Supplied by `Role::Source` (composed at Handle.pm:29) and not overridden by Handle. It derives from primary_key, which a handle answers as undef, so a derived table is never cachable.
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
- `handle_source.primary_key`: Answers undef unconditionally (Handle.pm:1472); a concrete table source answers differently.
- `handle_source.row_class`: Answers undef. Handle defines this unconditionally (Handle.pm:1473) as its Role::Source answer; it is not gated on the handle actually being nested. Reading it as the generic row-class answer would still be wrong: a concrete table source answers differently.
- `handle_source.source_db_moniker`: Renders the inner query as a literal subquery reference with binds. Croaks when the handle has no source, or when the subquery alias is not an identifier.
- `handle_source.source_has_aliases`: Always false for a derived table.
- `handle_source.source_orm_name`: Falls back to the default subquery alias.
- `iter.first`: Resets to the start and returns the first item. Same spelling as the handle and connection terminals, different receiver and different semantics.
- `iter.last`: Exhausts the generator and returns the last item, or undef.
- `iter.list`: Exhausts the generator and returns every item as a flat list.
- `iter.next`: Yields the next item, or empty once exhausted. Item identity is whatever the producing handle put in.
- `iter.ready`: True unless a readiness coderef was supplied and reports otherwise.
- `orm.connect`: Establishes and returns a connection.
- `orm.connection`: Returns the cached connection, connecting on first use.
- `orm.db`: Returns the DB definition. With an argument it sets it write-once, croaking if the DB is already set or was never set (ORM.pm:122-131).
- `orm.disconnect`: Lifecycle side effect.
- `orm.handle`: Delegates to the connection; the result's source comes from the arguments.
- `orm.reconnect`: Replaces and returns the connection.
- `row.cas`: Delegates to `_stored_handle->cas`. Handle::cas croaks on a forked handle, but a row builds its own handle from the connection.
- `row.check_pk`: Returns the receiving row when its source has a primary key, otherwise croaks (Role/Row.pm:183-187).
- `row.check_sync`: Returns `_check_stale`, which returns the receiving row (Row.pm:513-516) or croaks. It is not a boolean.
- `row.clone`: Produces another row of the same source identity.
- `row.conflate_args`: Returns a flat key/value list — field, value, source, dialect, affinity (Role/Row.pm:160) — not a hash container. It is a parenthesized list, so in scalar context the comma operator yields its last element.
- `row.connection`: Connection behind the row's data object.
- `row.delete`: Delegates to `_stored_handle->delete`. That handle comes from `connection->handle($self)` and is synchronous, so Handle::delete always takes its `return undef` branch (Handle.pm:2525-2528) — there is no statement handle to observe.
- `row.desynced_data`: Reads the DESYNC slot directly (Row.pm:118), so it is undef unless the row is desynced.
- `row.dialect`: Delegates to the connection's dialect.
- `row.discard`: Clears pending and desync state and returns the same row.
- `row.display`: Human-readable source name plus primary-key values; a string, never a row.
- `row.field`: Inflated single field value. This is the ordinary column accessor; named per-column accessors are an autorow feature and are not modeled here.
- `row.field_affinity`: Delegates to the source, passing the row's dialect.
- `row.field_is_desynced`: Per-field desync predicate.
- `row.fields`: Inflated field map merged from pending over stored.
- `row.follow`: Resolves the link and returns a handle on the link's *other* table (Role/Row.pm:344), so the receiver's row type does not survive. The handle comes from the connection and starts synchronous; a caller may refine it afterwards.
- `row.force_sync`: Clears the desync flag and returns the same row (Row.pm:269-273).
- `row.handle.copy`: Role/Row.pm:121-124 builds a handle scoped to the row's own source and row, then passes trailing arguments through Handle::handle. With no source or row argument the result stays on the receiver's own source, and starts synchronous.
- `row.handle.rebind`: Role/Row.pm:121-124 builds a handle scoped to the row's own source and row, then passes trailing arguments through Handle::handle. A source or row argument reaches Handle::handle and replaces that source, so the result can be a handle on another table.
- `row.has_field`: Delegates to the source; croaks without a field name.
- `row.has_pending`: State predicate.
- `row.in_storage`: State predicate.
- `row.insert`: Writes through the connection and returns the same row (Role/Row.pm:248). Croaks when already stored or when nothing is pending.
- `row.insert_or_save`: Dispatches to save when stored and insert when pending; both return the row. Croaks when there is nothing to write.
- `row.insert_related`: Inserts into the link's other table with the local values copied in, so the result belongs to that table, not the receiver's source.
- `row.is_desynced`: State predicate.
- `row.is_invalid`: State predicate.
- `row.is_stored`: Alias; upstream delegates to in_storage.
- `row.is_valid`: State predicate.
- `row.obtain`: follow($link)->one, so it may be undef. Croaks unless the link is unique. `follow` builds a fresh handle from the connection, which is synchronous, so this never yields an async placeholder.
- `row.pending_data`: Reads the PENDING slot directly (Row.pm:117), so it is undef when nothing is pending; `has_pending` guards the absent slot explicitly.
- `row.pending_field`: Inflated pending value for one field.
- `row.pending_fields`: Inflated pending field map.
- `row.primary_key_field_list`: Flat list of primary-key field names, empty when the source has no primary key.
- `row.primary_key_hash`: A `map` producing a flat key/value list (Role/Row.pm:105), not a hashref; `primary_key_hashref` is the reference form. In scalar context `map` yields the number of elements it produced, not a value — unlike the parenthesized list in conflate_args. Croaks through check_pk without a primary key.
- `row.primary_key_hashref`: The same pairs wrapped as a hashref (Role/Row.pm:106).
- `row.primary_key_value_list`: Raw stored primary-key values in field order; croaks through check_pk without a primary key.
- `row.raw_field`: Uninflated single field value.
- `row.raw_fields`: Uninflated field map; this is what by_id returns under data_only.
- `row.raw_pending_field`: Uninflated pending value for one field.
- `row.raw_pending_fields`: Uninflated pending field map.
- `row.raw_stored_field`: Uninflated stored value for one field.
- `row.raw_stored_fields`: Uninflated stored field map.
- `row.refresh`: Returns the refreshed row, or croaks when the row no longer exists (Row.pm:276-285). The handle comes from `_stored_handle`, so a row exposes no mode selector.
- `row.row_data`: Active row-data record.
- `row.row_data_obj`: Row-data object itself.
- `row.save`: Returns the same row on every path, including the early return when nothing is pending (Role/Row.pm:256-262).
- `row.siblings`: Returns a handle on the receiver's own source filtered to rows sharing the link's local values; includes the original row. The handle starts synchronous.
- `row.source`: Source behind the row. Same spelling as the handle accessor, different receiver and no clone behavior.
- `row.stored_data`: Reads the STORED slot directly (Row.pm:116), so it is undef when the row has no stored state.
- `row.stored_field`: Inflated stored value for one field.
- `row.stored_fields`: Inflated stored field map.
- `row.track_desync`: Constant true on the base row class.
- `row.update`: Stages the changes, saves, and returns the same row (Row.pm:365).
