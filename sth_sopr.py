import pandas as pd
import requests
import plotly.graph_objects as go
from configparser import ConfigParser
import yfinance as yf
import streamlit as st
from datetime import datetime

@st.cache_data(ttl=3600)
def get_sth_sopr_plot():
    # ====================== Configuration ======================
    config = ConfigParser()
    config.read('config.ini')

    try:
        DUNE_API_KEY = config['DUNE']['api_key'].strip().strip('"').strip("'")
    except KeyError:
        st.error("Clé API Dune manquante dans config.ini")
        return None

    # ====================== Requête Dune - STH-SOPR ======================
    try:
        raw_query_id = config['DUNE']['query_id'].strip().strip('"').strip("'")
        # Extraction de l'ID s'il s'agit d'une URL complète
        if '/' in raw_query_id:
            # On prend la partie après /queries/ ou le dernier segment numérique
            QUERY_ID = raw_query_id.split('/')[-1]
            if not QUERY_ID.isdigit(): # Cas où l'URL finit par /visualization_id
                 QUERY_ID = raw_query_id.split('/')[-2]
        else:
            QUERY_ID = raw_query_id
    except KeyError:
        QUERY_ID = "6987189" # Default/Fallback

    if not QUERY_ID.isdigit():
        st.error(f"L'ID de requête fourni ('{QUERY_ID}') n'est pas valide. Il doit s'agir d'un nombre.")
        return None

    url = f"https://api.dune.com/api/v1/query/{QUERY_ID}/results"
    headers = {
        "X-Dune-API-Key": DUNE_API_KEY,
        "Accept-Encoding": "identity" # Désactive gzip pour éviter les erreurs de décodage
    }

    try:
        response = requests.get(url, headers=headers, timeout=15)
        if response.status_code == 401:
            st.error("Erreur API Dune : 401 (Non autorisé). Veuillez vérifier que vous avez remplacé 'VOTRE_CLE_API_ICI' par une clé API valide dans le fichier config.ini.")
            return None
        elif response.status_code == 404:
            st.error(f"Erreur API Dune : 404 (Non trouvé). La requête avec l'ID {QUERY_ID} n'existe pas ou est privée. Veuillez vérifier le 'query_id' dans config.ini.")
            return None
        elif response.status_code == 400:
            try:
                err_msg = response.json().get('error', response.text)
            except:
                err_msg = response.text
            st.error(f"Erreur API Dune : 400 (Requête invalide). Détails : {err_msg}")
            return None
        elif response.status_code != 200:
            st.error(f"Erreur API Dune : {response.status_code}")
            return None

        data = response.json()['result']['rows']
    except requests.exceptions.RequestException as e:
        st.error(f"Erreur de connexion à l'API Dune : {e}")
        return None
    except Exception as e:
        st.error(f"Erreur lors du traitement des données Dune : {e}")
        return None

    df = pd.DataFrame(data)
    df['time'] = pd.to_datetime(df['time'])
    df = df.sort_values('time').reset_index(drop=True)
    df = df.rename(columns={'sopr': 'STH_SOPR'})

    # ====================== BTC Price ======================
    # On commence à la date minimale des données SOPR pour optimiser
    min_date = df['time'].min().strftime('%Y-%m-%d')
    btc = yf.download('BTC-USD', start=min_date, interval='1d', progress=False)

    if isinstance(btc.columns, pd.MultiIndex):
        close_prices = btc['Close']['BTC-USD']
    else:
        close_prices = btc['Close']

    btc_df = pd.DataFrame({'time': btc.index, 'BTC_Price': close_prices.values})
    btc_df['time'] = pd.to_datetime(btc_df['time']).dt.tz_localize(None)
    df['time'] = df['time'].dt.tz_localize(None)

    # Merge
    df = pd.merge(df, btc_df, on='time', how='left')

    # ====================== Graphique ======================
    fig = go.Figure()

    # STH-SOPR
    fig.add_trace(go.Scatter(
        x=df['time'],
        y=df['STH_SOPR'],
        mode='lines',
        name='STH-SOPR',
        line=dict(color='#00FFAA', width=2)
    ))

    # BTC Price
    fig.add_trace(go.Scatter(
        x=df['time'],
        y=df['BTC_Price'],
        mode='lines',
        name='Prix BTC',
        line=dict(color='rgba(255, 255, 255, 0.4)', width=1.5),
        yaxis="y2"
    ))

    # Ligne de base SOPR = 1
    fig.add_shape(
        type="line", line=dict(color="orange", width=1, dash="dash"),
        x0=df['time'].min(), x1=df['time'].max(), y0=1, y1=1
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
