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
    # Récupération de l'ID depuis config.ini
    try:
        QUERY_ID = config['DUNE']['query_id'].strip().strip('"').strip("'")
    except KeyError:
        # Fallback au cas où
        QUERY_ID = "6987189"

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

    df_inst = pd.DataFrame(data)

    # Identification de la colonne temporelle
    time_col = None
    for c in ['time', 'date', 'block_time', 'day']:
        if c in df_inst.columns:
            time_col = c
            break

    if not time_col:
        st.error(f"Colonne temporelle non trouvée. Colonnes dispos : {list(df_inst.columns)}")
        return None

    df_inst['date_merge'] = pd.to_datetime(df_inst[time_col]).dt.tz_localize(None)
    df_inst = df_inst.sort_values('date_merge').reset_index(drop=True)

    # Identification de la colonne de holdings
    # On cherche une colonne qui contient 'holding', 'btc', 'amount' ou 'total'
    val_col = None
    for c in df_inst.columns:
        low_c = c.lower()
        if 'holding' in low_c or 'total_btc' in low_c or 'btc_held' in low_c:
            val_col = c
            break

    if not val_col:
        # Fallback sur la première colonne numérique qui n'est pas la date
        for c in df_inst.columns:
            if c != time_col and pd.api.types.is_numeric_dtype(df_inst[c]):
                val_col = c
                break

    if not val_col:
        st.error(f"Colonne de données (Holdings) non trouvée. Colonnes dispos : {list(df_inst.columns)}")
        return None

    # ====================== BTC Price ======================
    min_date = df_inst['date_merge'].min().strftime('%Y-%m-%d')
    # On s'assure d'avoir au moins un peu d'historique si min_date est trop récent
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
    df = pd.merge(btc_df, df_inst, on='date_merge', how='left')
    df[val_col] = df[val_col].ffill()

    # ====================== Graphique ======================
    fig = make_subplots(
        rows=2, cols=1,
        shared_xaxes=True,
        vertical_spacing=0.08,
        row_heights=[0.65, 0.35],
        subplot_titles=("Prix du Bitcoin (USD)", "Holdings Institutionnels (BTC)")
    )

    fig.add_trace(go.Scatter(
        x=df['date_merge'], y=df['BTC_Price'],
        mode='lines', name='Prix BTC',
        line=dict(color='#00CCFF', width=2)
    ), row=1, col=1)

    fig.add_trace(go.Scatter(
        x=df['date_merge'], y=df[val_col],
        mode='lines', name='Holdings Institutionnels',
        line=dict(color='#00FFAA', width=3),
        fill='tozeroy'
    ), row=2, col=1)

    fig.update_layout(
        title="Bitcoin - Holdings Institutionnels vs Prix",
        xaxis_title="Date",
        template="plotly_dark",
        height=850,
        hovermode="x unified",
        legend=dict(x=0.01, y=0.99),
        paper_bgcolor='rgba(0,0,0,0)',
        plot_bgcolor='rgba(0,0,0,0)'
    )

    fig.update_yaxes(title_text="Prix BTC (USD)", type="log", row=1, col=1)
    fig.update_yaxes(title_text="BTC Institutionnels", row=2, col=1)

    return fig

if __name__ == "__main__":
    st.set_page_config(layout="wide")
    f = get_institutional_plot()
    if f:
        st.plotly_chart(f, use_container_width=True)
