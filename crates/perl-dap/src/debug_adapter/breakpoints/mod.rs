//! Breakpoint request handlers grouped by DAP responsibility.

use super::{
    AstBreakpointValidator, BreakpointLocation, BreakpointLocationsArguments,
    BreakpointLocationsResponseBody, BreakpointValidator, DapMessage, DataBreakpointInfoArguments,
    DataBreakpointInfoResponseBody, DataBreakpointRecord, DebugAdapter, HashMap,
    SetDataBreakpointsArguments, SetDataBreakpointsResponseBody, SetExceptionBreakpointsArguments,
    SetFunctionBreakpointsArguments, Value, Write, catalog_has_feature,
    is_valid_function_breakpoint_name, is_valid_set_variable_name, json, lock_or_recover,
};

mod data;
mod exception;
mod function;
mod line;
mod locations;
