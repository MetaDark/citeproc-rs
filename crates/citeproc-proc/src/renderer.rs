use crate::prelude::*;
use citeproc_io::output::LocalizedQuotes;
use citeproc_io::{Locator, NumericValue};
use csl::locale::Locale;
use csl::style::{Affixes, Formatting, Plural, Style};
use csl::terms::{GenderedTermSelector, LocatorType, TermForm, TextTermSelector};
use csl::variables::{NumberVariable, StandardVariable};
use csl::Atom;

enum GenericContext<'a, O: OutputFormat> {
    Ref(&'a RefContext<'a, O>),
    Cit(&'a CiteContext<'a, O>),
}

impl<O: OutputFormat> GenericContext<'_, O> {
    fn locator_type(&self) -> Option<LocatorType> {
        match self {
            Cit(ctx) => ctx.cite.locators.get(0).map(Locator::type_of),
            Ref(ctx) => ctx.locator_type,
        }
    }
    fn locale(&self) -> &Locale {
        match self {
            Cit(ctx) => ctx.locale,
            Ref(ctx) => ctx.locale,
        }
    }
    // fn get_number(&self, var: NumberVariable) -> Option<NumericValue> {
    //     match self {
    //         Cit(ctx) => ctx.get_number(var),
    //         Ref(ctx) => ctx.reference.number.get(var),
    //     }
    // }
}

use GenericContext::*;

pub struct Renderer<'a, O: OutputFormat> {
    ctx: GenericContext<'a, O>,
}

impl<O: OutputFormat> Renderer<'_, O> {
    pub fn refr<'c>(c: &'c RefContext<'c, O>) -> Renderer<'c, O> {
        Renderer {
            ctx: GenericContext::Ref(c),
        }
    }

    pub fn cite<'c>(c: &'c CiteContext<'c, O>) -> Renderer<'c, O> {
        Renderer {
            ctx: GenericContext::Cit(c),
        }
    }

    #[inline]
    fn fmt(&self) -> &O {
        match self.ctx {
            GenericContext::Cit(c) => &c.format,
            GenericContext::Ref(c) => c.format,
        }
    }

    pub fn number(
        &self,
        var: NumberVariable,
        val: NumericValue,
        f: Option<Formatting>,
        af: &Affixes,
    ) -> O::Build {
        let fmt = self.fmt();
        let options = IngestOptions {
            replace_hyphens: var.should_replace_hyphens(),
        };
        fmt.affixed_text(val.as_number(var.should_replace_hyphens()), f, af)
    }

    fn quotes(quo: bool) -> Option<LocalizedQuotes> {
        let q = LocalizedQuotes::Single(Atom::from("'"), Atom::from("'"));
        let quotes = if quo { Some(q) } else { None };
        quotes
    }

    pub fn text_variable(
        &self,
        var: StandardVariable,
        value: &str,
        f: Option<Formatting>,
        af: &Affixes,
        quo: bool,
        // sp, tc, disp
    ) -> O::Build {
        let fmt = self.fmt();
        let quotes = Renderer::<O>::quotes(quo);
        let options = IngestOptions {
            replace_hyphens: match var {
                StandardVariable::Ordinary(v) => v.should_replace_hyphens(),
                StandardVariable::Number(v) => v.should_replace_hyphens(),
            },
        };
        let b = fmt.ingest(value, options);
        let txt = fmt.with_format(b, f);

        let txt = match var {
            StandardVariable::Ordinary(v) => {
                let maybe_link = v.hyperlink(value);
                fmt.hyperlinked(txt, maybe_link)
            }
            StandardVariable::Number(_) => txt,
        };
        fmt.affixed_quoted(txt, &af, quotes.as_ref())
    }

    pub fn text_value(
        &self,
        value: &str,
        f: Option<Formatting>,
        af: &Affixes,
        quo: bool,
        // sp, tc, disp
    ) -> Option<O::Build> {
        if value.len() == 0 {
            return None;
        }
        let fmt = self.fmt();
        let quotes = Renderer::<O>::quotes(quo);
        let b = fmt.ingest(value, Default::default());
        let txt = fmt.with_format(b, f);
        Some(fmt.affixed_quoted(txt, af, quotes.as_ref()))
    }

    pub fn text_term(
        &self,
        term_selector: TextTermSelector,
        plural: bool,
        f: Option<Formatting>,
        af: &Affixes,
        quo: bool,
        // sp, tc, disp
    ) -> Option<O::Build> {
        let fmt = self.fmt();
        let locale = self.ctx.locale();
        let quotes = Renderer::<O>::quotes(quo);
        locale
            .get_text_term(term_selector, plural)
            .map(|val| fmt.affixed_text_quoted(val.to_owned(), f, af, quotes.as_ref()))
    }

    pub fn label(
        &self,
        var: NumberVariable,
        form: TermForm,
        num_val: NumericValue,
        plural: Plural,
        f: Option<Formatting>,
        af: &Affixes,
    ) -> Option<O::Build> {
        let fmt = self.fmt();
        let selector =
            GenderedTermSelector::from_number_variable(&self.ctx.locator_type(), var, form);
        let plural = match (num_val, plural) {
            (ref val, Plural::Contextual) => val.is_multiple(),
            (_, Plural::Always) => true,
            (_, Plural::Never) => false,
        };
        selector.and_then(|sel| {
            self.ctx
                .locale()
                .get_text_term(TextTermSelector::Gendered(sel), plural)
                .map(|val| fmt.affixed_text(val.to_owned(), f, &af))
        })
    }
}
