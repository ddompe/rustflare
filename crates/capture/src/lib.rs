//! File output (SER sequences, PNG stills) and flat-field logic.

pub mod flats;
pub mod ser;
pub mod sink;
pub mod still;

pub use flats::{FlatAccumulator, FlatDecision, FlatTarget};
pub use ser::SerWriter;
pub use sink::{BurstSpec, Finished, Format, FrameSink};
pub use still::write_png;
