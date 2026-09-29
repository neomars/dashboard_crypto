//! Cœur du Dashboard Crypto : accès aux données (Yahoo Finance, Dune Analytics,
//! alternative.me, mempool.space), calcul des indicateurs et construction des
//! figures Plotly (JSON) affichées par l'interface.

pub mod catalog;
pub mod config;
pub mod data;
pub mod dune;
pub mod figure;
pub mod indicators;
pub mod pdf;
pub mod series;
pub mod simulator;

pub use data::DataProvider;
