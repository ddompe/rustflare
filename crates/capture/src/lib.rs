//! File output (SER sequences, PNG stills) and flat-field logic.

pub mod debayer;
pub mod fits;
pub mod flats;
pub mod prominence;
pub mod ser;
pub mod sink;
pub mod still;

pub use flats::{DarkFrame, FlatAccumulator, FlatDecision, FlatField, FlatSetting, FlatTarget, SharedFlat};
pub use ser::SerWriter;
pub use sink::{BurstSpec, Finished, Format, FrameSink};
pub use still::{read_png, write_png, write_png_rgb};
pub use fits::{write_fits, FitsMeta};
