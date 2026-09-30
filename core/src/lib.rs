//! Cœur du Dashboard Crypto : accès aux données ouvertes (Yahoo Finance, BGeometrics, OKX,
//! alternative.me, mempool.space), calcul des indicateurs et construction des
//! figures Plotly (JSON) affichées par l'interface.

pub mod bgeometrics;
pub mod catalog;
pub mod config;
pub mod data;
pub mod figure;
pub mod indicators;
pub mod okx;
pub mod pdf;
pub mod sec;
pub mod series;
pub mod simulator;
pub mod table;

pub use data::DataProvider;
