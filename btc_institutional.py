import pandas as pd
import plotly.graph_objects as go
from plotly.subplots import make_subplots
import streamlit as st

from config_manager import get_dune_query_id
from data_provider import get_dune_query_results, get_ticker_history

QUERY_ID = get_dune_query_id('institutional', '3382000')  # surchargable via DUNE_QUERY_INSTITUTIONAL ou config.ini [DUNE_QUERIES]

@st.cache_data(ttl=3600)
def get_institutional_plot():
    # ====================== Query Dune ======================
    df_raw, error = get_dune_query_results(QUERY_ID)
    if error:
        st.error(error)
        return None
    if df_raw is None or df_raw.empty:
        st.warning("Aucune donnée retournée par la requête Dune.")
        return None

    # Identification des colonnes
    time_col = next((c for c in ['time', 'date', 'block_time', 'day'] if c in df_raw.columns), None)
    ticker_col = next((c for c in ['etf_ticker', 'ticker', 'symbol'] if c in df_raw.columns), None)
    val_col = next((c for c in ['tvl', 'holding', 'btc_held', 'amount'] if c in df_raw.columns), None)

    if not time_col or not ticker_col or not val_col:
        st.error(f"Structure de données Dune inattendue. Colonnes : {list(df_raw.columns)}")
        return None

    df_raw[time_col] = pd.to_datetime(df_raw[time_col]).dt.tz_localize(None)

    # Pivot pour avoir une colonne par émetteur (ETF)
    df_pivot = df_raw.pivot(index=time_col, columns=ticker_col, values=val_col).ffill().fillna(0)
    df_pivot.index.name = 'date_merge'

    # Calcul du total pour le merge avec le prix
    df_pivot['Total_Institutional'] = df_pivot.sum(axis=1)

    # ====================== BTC Price ======================
    min_date = df_pivot.index.min().strftime('%Y-%m-%d')
    btc = get_ticker_history('BTC-USD', start=min_date)

    if btc.empty:
        st.error("Impossible de récupérer les prix BTC via yfinance.")
        return None

    close_prices = btc['Close']
    btc_df = pd.DataFrame({'date_merge': btc.index, 'BTC_Price': close_prices.values})
    btc_df['date_merge'] = pd.to_datetime(btc_df['date_merge']).dt.tz_localize(None)

    # ====================== Fusion ======================
    df = pd.merge(btc_df, df_pivot, on='date_merge', how='left').ffill().fillna(0)

    # ====================== Graphique ======================
    fig = make_subplots(
        rows=2, cols=1,
        shared_xaxes=True,
        vertical_spacing=0.08,
        row_heights=[0.65, 0.35],
        subplot_titles=("Prix du Bitcoin (USD)", "Breakdown des Holdings Institutionnels (BTC)")
    )

    # BTC Price
    fig.add_trace(go.Scatter(
        x=df['date_merge'], y=df['BTC_Price'],
        mode='lines', name='Prix BTC',
        line=dict(color='#00CCFF', width=2)
    ), row=1, col=1)

    # Stacked Area pour les émetteurs
    # On exclut 'Total_Institutional' et 'BTC_Price' des émetteurs
    tickers = [c for c in df_pivot.columns if c != 'Total_Institutional']

    for ticker in tickers:
        fig.add_trace(go.Scatter(
            x=df['date_merge'],
            y=df[ticker],
            mode='lines',
            name=ticker,
            stackgroup='one', # Création du graphique empilé
            line=dict(width=0.5),
            hovertemplate='%{y:,.0f} BTC'
        ), row=2, col=1)

    fig.update_layout(
        title="Bitcoin - Holdings Institutionnels Détallés vs Prix",
        xaxis_title="Date",
        template="plotly_dark",
        height=900,
        hovermode="x unified",
        legend=dict(
            orientation="v",
            yanchor="top",
            y=1,
            xanchor="left",
            x=1.02
        ),
        margin=dict(r=150), # Ajout de marge à droite pour la légende
        paper_bgcolor='rgba(0,0,0,0)',
        plot_bgcolor='rgba(0,0,0,0)'
    )

    fig.update_yaxes(title_text="Prix BTC (USD)", type="log", row=1, col=1)
    fig.update_yaxes(title_text="Holdings (BTC)", row=2, col=1)

    return fig

if __name__ == "__main__":
    st.set_page_config(layout="wide")
    f = get_institutional_plot()
    if f:
        st.plotly_chart(f, use_container_width=True)
