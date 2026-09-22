import pandas as pd
import plotly.graph_objects as go
import streamlit as st

from config_manager import get_dune_query_id
from data_provider import get_dune_query_results, get_ticker_history

QUERY_ID = get_dune_query_id('sopr', '6764134')  # ID fixe pour LTH/STH SOPR (surchargable via DUNE_QUERY_SOPR ou config.ini [DUNE_QUERIES])

@st.cache_data(ttl=3600)
def get_sth_sopr_plot():
    # ====================== Requête Dune - SOPR ======================
    df, error = get_dune_query_results(QUERY_ID)
    if error:
        st.error(error)
        return None
    if df is None or df.empty:
        st.warning("Aucune donnée retournée par la requête Dune.")
        return None

    # Identification de la colonne temporelle
    time_col = None
    for c in ['time', 'block_time', 'date', 'day']:
        if c in df.columns:
            time_col = c
            break

    if not time_col:
        st.error(f"Impossible de trouver une colonne temporelle dans les données. Colonnes dispos : {list(df.columns)}")
        return None

    df[time_col] = pd.to_datetime(df[time_col])
    df = df.sort_values(time_col).reset_index(drop=True)

    # Identification des colonnes SOPR (STH, LTH ou simplement SOPR)
    sopr_cols = [c for c in df.columns if 'sopr' in c.lower()]
    if not sopr_cols:
        st.error(f"Aucune colonne SOPR détectée. Colonnes dispos : {list(df.columns)}")
        return None

    # ====================== BTC Price ======================
    min_date = df[time_col].min().strftime('%Y-%m-%d')
    btc = get_ticker_history('BTC-USD', start=min_date)
    if btc.empty:
        st.error("Impossible de récupérer les prix BTC via yfinance.")
        return None

    close_prices = btc['Close']
    btc_df = pd.DataFrame({'time_merge': btc.index, 'BTC_Price': close_prices.values})
    btc_df['time_merge'] = pd.to_datetime(btc_df['time_merge']).dt.tz_localize(None)
    df[time_col] = df[time_col].dt.tz_localize(None)

    # Merge
    df = pd.merge(df, btc_df, left_on=time_col, right_on='time_merge', how='left')

    # ====================== Graphique ======================
    fig = go.Figure()

    colors = ['#00FFAA', '#FF00AA', '#AAFF00', '#00AAFF']
    for i, col in enumerate(sopr_cols):
        fig.add_trace(go.Scatter(
            x=df[time_col],
            y=df[col],
            mode='lines',
            name=col.upper().replace('_', ' '),
            line=dict(color=colors[i % len(colors)], width=2)
        ))

    # BTC Price
    fig.add_trace(go.Scatter(
        x=df[time_col],
        y=df['BTC_Price'],
        mode='lines',
        name='Prix BTC',
        line=dict(color='rgba(255, 255, 255, 0.4)', width=1.5),
        yaxis="y2"
    ))

    # Ligne de base SOPR = 1
    fig.add_shape(
        type="line", line=dict(color="orange", width=1, dash="dash"),
        x0=df[time_col].min(), x1=df[time_col].max(), y0=1, y1=1
    )

    fig.update_layout(
        title="Bitcoin - STH-SOPR (Short Term Holder Output Profit Ratio)",
        xaxis_title="Date",
        yaxis=dict(
            title="STH-SOPR",
            type="log",
            showgrid=True,
            gridcolor='rgba(128, 128, 128, 0.2)'
        ),
        yaxis2=dict(
            title="Prix BTC (USD)",
            type="log",
            overlaying='y',
            side='right',
            showgrid=False
        ),
        template="plotly_dark",
        height=700,
        hovermode="x unified",
        legend=dict(x=0.01, y=0.99),
        paper_bgcolor='rgba(0,0,0,0)',
        plot_bgcolor='rgba(0,0,0,0)'
    )

    return fig

if __name__ == "__main__":
    # Test
    fig = get_sth_sopr_plot()
    if fig:
        fig.show()
