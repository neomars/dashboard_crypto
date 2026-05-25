import pandas as pd
import yfinance as yf
import plotly.graph_objects as go
from plotly.subplots import make_subplots
from configparser import ConfigParser
import requests
import streamlit as st
from datetime import datetime

@st.cache_data(ttl=3600)
def get_institutional_plot():
    # ====================== Configuration ======================
    config = ConfigParser()
    config.read('config.ini')
    try:
        DUNE_API_KEY = config['DUNE']['api_key'].strip().strip('"').strip("'")
    except KeyError:
        st.error("Clé API Dune manquante dans config.ini")
        return None

    # ====================== Query Dune ======================
    QUERY_ID = "3382000"

    url = f"https://api.dune.com/api/v1/query/{QUERY_ID}/results"
    headers = {
        "X-Dune-API-Key": DUNE_API_KEY,
        "Accept-Encoding": "identity"
    }

    try:
        response = requests.get(url, headers=headers, timeout=15)
        if response.status_code != 200:
            st.error(f"Erreur Dune API : {response.status_code}. Vérifiez votre clé API et l'ID de requête ({QUERY_ID}).")
            return None
        data = response.json()['result']['rows']
    except Exception as e:
        st.error(f"Erreur lors de la récupération des données Dune : {e}")
        return None

    if not data:
        st.warning("Aucune donnée retournée par la requête Dune.")
        return None

    df_raw = pd.DataFrame(data)

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
    btc = yf.download('BTC-USD', start=min_date, interval='1d', progress=False)

    if btc.empty:
        st.error("Impossible de récupérer les prix BTC via yfinance.")
        return None

    if isinstance(btc.columns, pd.MultiIndex):
        close_prices = btc['Close']['BTC-USD']
    else:
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
