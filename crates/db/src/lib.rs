#[macro_use]
extern crate log;

mod cite;
mod xml;

pub use cite::*;
pub use xml::*;

pub fn safe_default(db: &mut (impl cite::CiteDatabase + xml::LocaleDatabase + xml::StyleDatabase)) {
    use std::sync::Arc;
    // TODO: more salsa::inputs
    db.set_style(Default::default());
    db.set_all_keys(Default::default());
    db.set_all_uncited(Default::default());
    db.set_cluster_ids(Arc::new(vec![]));
    db.set_locale_input_langs(Default::default());
}
