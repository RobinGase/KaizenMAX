/// DOM selectors for Gemini web interface (gemini.google.com).
///
/// These are fallback-chain ready: primary selector first, then alternatives.
/// Update here when Google changes the DOM; executors do not hardcode selectors.

pub const APP_URL: &str = "https://gemini.google.com/app";

/// Primary prompt input textarea
pub const PROMPT_TEXTAREA: &str = "textarea";
/// Fallback — rich text editor div
pub const PROMPT_TEXTAREA_FALLBACK: &str = "div[contenteditable='true']";

/// Submit / send message button
pub const SUBMIT_BUTTON: &str = "button[aria-label='Send message']";
pub const SUBMIT_BUTTON_FALLBACK: &str = "button[data-testid='send-button']";

/// Container holding the latest model response
pub const RESPONSE_CONTAINER: &str = "div.response-content";
pub const RESPONSE_CONTAINER_FALLBACK: &str = "model-response";

/// Marker that indicates the user is not logged in (Google sign-in redirect link)
pub const AUTH_REQUIRED_MARKER: &str = "[href*='accounts.google.com']";
