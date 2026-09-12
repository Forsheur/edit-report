//! Adapters between the outside world and `edit-report-core`.
//!
//! The core takes decoded frames and parsed metadata and knows nothing else.
//! These modules are what turn a `.zip`, a video file and a Python interpreter
//! into those inputs. They live in a library rather than inside the binary so
//! development tools can drive the same pipeline the binary drives — a second
//! copy of the decoding path would be a second thing to keep true.

pub mod bundle_dir;
pub mod decode;
pub mod pyverify;
