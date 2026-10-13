//! Moteur de l'activité de trading du Dashboard Crypto.
//!
//! Il est séparé des indicateurs de marché du tableau de bord
//! (`dashboard-core`) : il ne dépend pas d'eux, ne lit pas leurs données et
//! ne les utilise pas pour décider. Il a ses propres données (bougies et
//! funding de Poloniex, la plateforme d'exécution), ses propres indicateurs de
//! signal et ses propres règles de risque. Le tableau de bord ne fait
//! qu'afficher ses résultats, dans une section « Trading » distincte.
//!
//! - [`feed`] et [`poloniex`] : données publiques de Poloniex (sans clé) ;
//! - [`ta`] : indicateurs de signal (moyennes mobiles, ATR, canaux, volatilité) ;
//! - [`strategy`] : module tendance (ensemble Donchian, croisement EMA) ;
//! - [`risk`] : cible de volatilité, plafond de levier, stop suiveur, coupe-circuits ;
//! - [`backtest`] et [`metrics`] : simulation sur l'historique et mesures de performance ;
//! - [`signals`] : état du signal à la dernière clôture ;
//! - [`chart`] et [`catalog`] : figures et pages affichées par le tableau de bord.
//!
//! L'exécution réelle des ordres (clés API Poloniex) n'est pas encore écrite :
//! elle réutilisera les mêmes signaux et règles de risque.

pub mod accumulation;
pub mod backtest;
pub mod candle_store;
pub mod catalog;
pub mod chart;
pub mod contract;
pub mod feed;
pub mod fibo;
pub mod fibo_chart;
pub mod fibo_data;
pub mod fibo_report;
pub mod leveraged;
pub mod market;
pub mod metrics;
pub mod mvrv;
pub mod poloniex;
pub mod risk;
pub mod signals;
pub mod strategy;
pub mod ta;
pub mod validation;

pub use feed::MarketFeed;
