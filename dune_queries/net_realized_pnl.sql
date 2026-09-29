-- Bitcoin : profit et perte réalisés par semaine (USD)
-- Utilisé par net_realized_pnl.py. À créer sur https://dune.com (New query),
-- puis renseigner son ID dans config.ini :
--   [DUNE_QUERIES]
--   net_realized_pnl = <ID>
--
-- Chaque UTXO dépensé réalise (prix du jour de dépense - prix du jour de
-- création) x montant. On agrège d'abord par (jour de dépense, jour de
-- création) : sur une même paire de jours, l'écart de prix a un signe unique,
-- donc le classement profit/perte reste exact.
--
-- Limites : prix journaliers moyens (pas au bloc près), pas d'ajustement par
-- entité (contrairement à Glassnode), et les UTXO créés avant le début de
-- l'historique de prix de prices.usd sont ignorés.
WITH daily_price AS (
    SELECT CAST(date_trunc('day', minute) AS date) AS day,
           AVG(price) AS price
    FROM prices.usd
    WHERE blockchain IS NULL
      AND symbol = 'BTC'
    GROUP BY 1
),
block_day AS (
    SELECT height,
           CAST(date_trunc('day', time) AS date) AS day
    FROM bitcoin.blocks
),
spent AS (
    SELECT CAST(date_trunc('day', i.block_time) AS date) AS spend_day,
           b.day AS create_day,
           SUM(i.value) AS btc
    FROM bitcoin.inputs i
    JOIN block_day b ON b.height = i.spent_block_height
    WHERE i.block_time >= TIMESTAMP '2023-01-01'
      AND NOT i.is_coinbase
    GROUP BY 1, 2
),
pnl AS (
    SELECT s.spend_day,
           s.btc * (ps.price - pc.price) AS realized_usd
    FROM spent s
    JOIN daily_price ps ON ps.day = s.spend_day
    JOIN daily_price pc ON pc.day = s.create_day
)
SELECT date_trunc('week', spend_day) AS week,
       SUM(CASE WHEN realized_usd > 0 THEN realized_usd ELSE 0 END) AS realized_profit_usd,
       SUM(CASE WHEN realized_usd < 0 THEN -realized_usd ELSE 0 END) AS realized_loss_usd
FROM pnl
GROUP BY 1
ORDER BY 1
