//! Canonical htmx protocol and attribute metadata.

/// Direction in which an htmx header is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HtmxHeaderDirection {
    /// Browser-to-server request header.
    Request,
    /// Server-to-browser response header.
    Response,
    /// Header whose meaning depends on whether it is used on a request or response.
    RequestAndResponse,
}

impl HtmxHeaderDirection {
    pub(crate) const fn detail(self) -> &'static str {
        match self {
            Self::Request => "htmx request header",
            Self::Response => "htmx response header",
            Self::RequestAndResponse => "htmx request and response header",
        }
    }
}

/// One canonical htmx request or response header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HtmxHeaderSpec {
    /// Canonically cased header name.
    pub name: &'static str,
    /// Direction in which the header is used.
    pub direction: HtmxHeaderDirection,
    /// Concise user-facing documentation.
    pub documentation: &'static str,
}

macro_rules! header {
    ($name:literal, $direction:ident, $documentation:expr) => {
        HtmxHeaderSpec {
            name: $name,
            direction: HtmxHeaderDirection::$direction,
            documentation: $documentation,
        }
    };
}

/// Current unique htmx request and response headers.
pub const HTMX_HEADERS: &[HtmxHeaderSpec] = &[
    header!(
        "HX-Boosted",
        Request,
        "Indicates that the request came from an element using `hx-boost`."
    ),
    header!(
        "HX-Current-URL",
        Request,
        "Contains the browser URL when the htmx request was issued."
    ),
    header!(
        "HX-History-Restore-Request",
        Request,
        "Set to `true` for a history restoration request after a local history-cache miss."
    ),
    header!("HX-Location", Response, "Performs a client-side redirect without a full page reload."),
    header!("HX-Prompt", Request, "Contains the user's response to an `hx-prompt` dialog."),
    header!("HX-Push-Url", Response, "Pushes a URL into the browser history stack."),
    header!(
        "HX-Redirect",
        Response,
        "Redirects the browser to a new location with a full page reload."
    ),
    header!("HX-Refresh", Response, "When set to `true`, causes a full page refresh."),
    header!("HX-Replace-Url", Response, "Replaces the current URL in the browser location bar."),
    header!("HX-Request", Request, "Set to `true` on requests issued by htmx."),
    header!(
        "HX-Reselect",
        Response,
        "Selects which part of the response will be swapped, overriding `hx-select`."
    ),
    header!(
        "HX-Reswap",
        Response,
        "Overrides the response swap strategy using an `hx-swap` value."
    ),
    header!(
        "HX-Retarget",
        Response,
        "Uses a CSS selector to override the element that receives the swapped content."
    ),
    header!("HX-Target", Request, "Contains the `id` of the target element when one exists."),
    header!(
        "HX-Trigger",
        RequestAndResponse,
        "Request: contains the `id` of the triggering element. Response: triggers client-side events when the response is received."
    ),
    header!(
        "HX-Trigger-After-Settle",
        Response,
        "Triggers client-side events after the settle step."
    ),
    header!("HX-Trigger-After-Swap", Response, "Triggers client-side events after the swap step."),
    header!(
        "HX-Trigger-Name",
        Request,
        "Contains the `name` of the triggering element when one exists."
    ),
];

/// Shape of an htmx attribute name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HtmxAttributeFamily {
    /// A complete, fixed attribute name.
    Fixed,
    /// Prefix for the dynamic `hx-on:<event>` event-handler family.
    EventHandler,
}

/// One canonical htmx attribute or dynamic attribute family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HtmxAttributeSpec {
    /// Canonical lowercase attribute name or family prefix.
    pub name: &'static str,
    /// Whether the entry is fixed or introduces a dynamic name family.
    pub family: HtmxAttributeFamily,
    /// Concise user-facing documentation.
    pub documentation: &'static str,
    /// Whether the attribute is deprecated by htmx.
    pub deprecated: bool,
}

impl HtmxAttributeSpec {
    pub(crate) const fn detail(self) -> &'static str {
        if self.deprecated {
            "htmx attribute (deprecated)"
        } else {
            match self.family {
                HtmxAttributeFamily::Fixed => "htmx attribute",
                HtmxAttributeFamily::EventHandler => "htmx event-handler attribute family",
            }
        }
    }
}

macro_rules! attribute {
    ($name:literal, $documentation:literal) => {
        HtmxAttributeSpec {
            name: $name,
            family: HtmxAttributeFamily::Fixed,
            documentation: $documentation,
            deprecated: false,
        }
    };
}

macro_rules! deprecated_attribute {
    ($name:literal, $documentation:literal) => {
        HtmxAttributeSpec {
            name: $name,
            family: HtmxAttributeFamily::Fixed,
            documentation: $documentation,
            deprecated: true,
        }
    };
}

/// Current core and additional htmx attributes.
///
/// `hx-on:` represents the dynamic event-handler family. The deprecated plain
/// `hx-on` spelling is deliberately not a completion candidate.
pub const HTMX_ATTRIBUTES: &[HtmxAttributeSpec] = &[
    attribute!("hx-boost", "Progressively enhances links and forms with htmx requests."),
    attribute!("hx-confirm", "Prompts for confirmation before issuing the request."),
    attribute!("hx-delete", "Issues an HTTP DELETE request to the specified URL."),
    attribute!("hx-disable", "Disables htmx processing for the element and its descendants."),
    attribute!(
        "hx-disabled-elt",
        "Selects elements to disable while an htmx request is in flight."
    ),
    attribute!("hx-disinherit", "Prevents selected inherited htmx attributes from propagating."),
    attribute!("hx-encoding", "Sets the request encoding, including multipart form uploads."),
    attribute!("hx-ext", "Enables or ignores named htmx extensions for an element."),
    attribute!("hx-get", "Issues an HTTP GET request to the specified URL."),
    attribute!("hx-headers", "Adds headers to the htmx request."),
    attribute!("hx-history", "Controls whether sensitive page state may be cached in history."),
    attribute!(
        "hx-history-elt",
        "Selects the element whose content is saved and restored for history."
    ),
    attribute!("hx-include", "Includes values from additional selected elements in the request."),
    attribute!("hx-indicator", "Selects the request indicator element."),
    attribute!("hx-inherit", "Forces selected htmx attributes to be inherited."),
    HtmxAttributeSpec {
        name: "hx-on:",
        family: HtmxAttributeFamily::EventHandler,
        documentation: "Starts an htmx event-handler attribute; append the event name.",
        deprecated: false,
    },
    attribute!("hx-params", "Filters the parameters submitted with an htmx request."),
    attribute!("hx-patch", "Issues an HTTP PATCH request to the specified URL."),
    attribute!("hx-post", "Issues an HTTP POST request to the specified URL."),
    attribute!("hx-preserve", "Keeps an element unchanged across HTML replacement."),
    attribute!("hx-prompt", "Prompts the user and sends the response with the request."),
    attribute!("hx-push-url", "Pushes a URL into browser history after an htmx response."),
    attribute!("hx-put", "Issues an HTTP PUT request to the specified URL."),
    attribute!("hx-replace-url", "Replaces the current browser-history URL after a response."),
    attribute!("hx-request", "Configures request options such as timeout and credentials."),
    attribute!("hx-select", "Selects the response fragment to swap into the target."),
    attribute!("hx-select-oob", "Selects additional response fragments for out-of-band swaps."),
    attribute!("hx-swap", "Controls how and when response content is swapped."),
    attribute!("hx-swap-oob", "Marks content for an out-of-band swap."),
    attribute!("hx-sync", "Coordinates concurrent htmx requests between selected elements."),
    attribute!("hx-target", "Selects the element that receives the response content."),
    attribute!("hx-trigger", "Specifies the events or polling expression that trigger a request."),
    attribute!("hx-validate", "Enables HTML form validation before the htmx request."),
    attribute!("hx-vals", "Adds values to the parameters submitted with the request."),
    deprecated_attribute!(
        "hx-vars",
        "Deprecated expression-based request values; use `hx-vals` instead."
    ),
];

/// ASCII-case-insensitive `value.starts_with(prefix)`.
pub(crate) fn starts_with_ignore_ascii_case(value: &str, prefix: &str) -> bool {
    value.get(..prefix.len()).is_some_and(|head| head.eq_ignore_ascii_case(prefix))
}
