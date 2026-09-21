//! The result: one row per **column** of the crosstab — one per cell — with its two embedding
//! coordinates, named `umap.1` and `umap.2` as the R operator names them, keyed by `.ci`.
//!
//! One table only: a second relation joined on nothing is a cross join against the crosstab
//! (flowsom 0.1.1 learned this), so nothing else is returned.
use std::io::Write;

use anyhow::{Result, anyhow};

use crate::tson::TsonWriter;

pub struct ColSpec<'a> {
    pub name: &'a str,
    pub ty: &'a str, // "double" | "int32" | "string"
}

pub fn write_header<W: Write>(
    w: &mut TsonWriter<W>,
    table_name: &str,
    n_rows: usize,
    cols: &[ColSpec],
    n_tables: usize,
) -> Result<()> {
    w.map(3)?;
    w.key("kind")?;
    w.str("OperatorResult")?;
    w.key("tables")?;
    w.list(n_tables)?;

    w.map(4)?;
    w.key("kind")?;
    w.str("Table")?;
    w.key("nRows")?;
    w.i32(i32::try_from(n_rows).map_err(|_| {
        anyhow!(
            "the result would have {n_rows} rows, more than a Tercen table can hold (i32::MAX). \
             Project fewer cells, or split the step."
        )
    })?)?;
    w.key("properties")?;
    w.map(4)?;
    w.key("kind")?;
    w.str("TableProperties")?;
    w.key("name")?;
    w.str(table_name)?;
    w.key("sortOrder")?;
    w.list(0)?;
    w.key("ascending")?;
    w.bool(false)?;
    w.key("columns")?;
    w.list(cols.len())?;
    Ok(())
}

pub fn write_column_header<W: Write>(
    w: &mut TsonWriter<W>,
    c: &ColSpec,
    n_rows: usize,
) -> Result<()> {
    w.map(6)?;
    w.key("kind")?;
    w.str("Column")?;
    w.key("name")?;
    w.str(c.name)?;
    w.key("type")?;
    w.str(c.ty)?;
    w.key("nRows")?;
    w.i32(n_rows as i32)?;
    w.key("size")?;
    w.i32(n_rows as i32)?;
    w.key("values")?;
    Ok(())
}

/// The per-cell table: `umap.1`, `umap.2`, `.ci`. `x` and `y` are in `.ci` order.
pub fn write_embedding<W: Write>(
    w: &mut TsonWriter<W>,
    table_name: &str,
    namespace: &str,
    x: &[f64],
    y: &[f64],
) -> Result<()> {
    let n = x.len();
    let x_name = format!("{namespace}.umap.1");
    let y_name = format!("{namespace}.umap.2");
    let cols = [
        ColSpec {
            name: &x_name,
            ty: "double",
        },
        ColSpec {
            name: &y_name,
            ty: "double",
        },
        ColSpec {
            name: ".ci",
            ty: "int32",
        },
    ];
    write_header(w, table_name, n, &cols, 1)?;
    write_column_header(w, &cols[0], n)?;
    w.f64_list(x)?;
    write_column_header(w, &cols[1], n)?;
    w.f64_list(y)?;
    write_column_header(w, &cols[2], n)?;
    w.i32_list(&(0..n as i32).collect::<Vec<_>>())?;
    Ok(())
}

/// Close the result. One relation, so no joins to declare.
pub fn write_footer<W: Write>(w: &mut TsonWriter<W>) -> Result<()> {
    w.key("joinOperators")?;
    w.list(0)?;
    w.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_result_bigger_than_an_i32_is_an_error_not_a_panic() {
        let mut w = TsonWriter::new(Vec::new()).unwrap();
        let cols = [ColSpec {
            name: ".ci",
            ty: "int32",
        }];
        let e = write_header(&mut w, "cells", i32::MAX as usize + 1, &cols, 1).unwrap_err();
        assert!(e.to_string().contains("more than a Tercen table can hold"));
    }

    #[test]
    fn the_embedding_decodes_as_a_result_with_three_typed_columns() {
        let mut w = TsonWriter::new(Vec::new()).unwrap();
        write_embedding(&mut w, "t", "ds0", &[1.0, 2.0], &[3.0, 4.0]).unwrap();
        write_footer(&mut w).unwrap();
        let bytes = w.into_inner();
        let v = rustson::decode(std::io::Cursor::new(&bytes[..])).unwrap();
        let rustson::Value::MAP(m) = v else { panic!() };
        let rustson::Value::LST(tables) = &m["tables"] else {
            panic!()
        };
        let rustson::Value::MAP(t) = &tables[0] else {
            panic!()
        };
        let rustson::Value::LST(cols) = &t["columns"] else {
            panic!()
        };
        let names: Vec<String> = cols
            .iter()
            .map(|c| {
                let rustson::Value::MAP(c) = c else { panic!() };
                let rustson::Value::STR(n) = &c["name"] else {
                    panic!()
                };
                n.clone()
            })
            .collect();
        assert_eq!(names, ["ds0.umap.1", "ds0.umap.2", ".ci"]);
    }
}
