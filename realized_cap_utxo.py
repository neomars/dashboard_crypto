import pandas as pd
import plotly.graph_objects as go
import requests
import streamlit as st
from datetime import datetime
import numpy as np
import time
from config_manager import get_dune_api_key

@st.cache_data(ttl=7200)  # Cache 2 heures (adapté au tier gratuit)
def get_dune_utxo_realized_cap():
    """Récupère les données UTXO Age Bands depuis Dune Analytics"""
    query_id = 7611528  # Query légère : Bitcoin UTXO Age Bands (mensuelle) - https://dune.com/queries/7611528

    api_key = get_dune_api_key()
    if not api_key:
        st.warning("Clé API Dune manquante dans la configuration.")
        return get_fallback_data()

    headers = {
        "x-dune-api-key": api_key,
        "accept": "application/json"
    }

    try:
        # 1. Essayer de récupérer les résultats existants
        url = f"https://api.dune.com/api/v1/query/{query_id}/results"
        response = requests.get(url, headers=headers, timeout=60)

        if response.status_code == 200:
            data = response.json()
            if 'result' in data and 'rows' in data['result']:
                df = pd.DataFrame(data['result']['rows'])
                return _process_dune_df(df)

        # 2. Si pas de résultats → exécuter la query
        st.info("Exécution de la query Dune en cours (peut prendre 30-60s)...")
        return execute_dune_query(query_id, api_key)

    except Exception as e:
        st.error(f"Erreur lors de la récupération Dune : {e}")
        return get_fallback_data()

def execute_dune_query(query_id: int, api_key: str):
    """Exécute la query et attend les résultats (gestion timeout)"""
    execution_url = f"https://api.dune.com/api/v1/query/{query_id}/execute"
    headers = {"x-dune-api-key": api_key, "Content-Type": "application/json"}

    try:
        exec_resp = requests.post(execution_url, headers=headers, timeout=30)
        if exec_resp.status_code != 200:
            st.error(f"Erreur exécution Dune : {exec_resp.status_code}")
            return get_fallback_data()

        execution_id = exec_resp.json().get('execution_id')

        # Polling des résultats
        for _ in range(40):  # ~80 secondes max
            result_url = f"https://api.dune.com/api/v1/execution/{execution_id}/results"
            resp = requests.get(result_url, headers={"x-dune-api-key": api_key}, timeout=30)

            if resp.status_code == 200:
                result_data = resp.json()
                if result_data.get('state') == 'QUERY_STATE_COMPLETED':
                    df = pd.DataFrame(result_data['result']['rows'])
                    return _process_dune_df(df)
                elif result_data.get('state') in ['QUERY_STATE_FAILED', 'QUERY_STATE_CANCELLED']:
                    st.error("La query Dune a échoué.")
                    break

            time.sleep(2)

        st.error("Timeout Dune (même en exécution). Utilisation des données simulées.")
        return get_fallback_data()

    except Exception as e:
        st.error(f"Erreur exécution Dune : {e}")
        return get_fallback_data()

def _process_dune_df(df: pd.DataFrame):
    """Adapte le DataFrame selon les colonnes renvoyées par la query"""
    if df.empty:
        return get_fallback_data()

    # Renommage selon les colonnes courantes sur Dune (à ajuster si besoin)
    if 'month' in df.columns:
        df = df.rename(columns={'month': 'date'})
    elif 'time' in df.columns:
        df = df.rename(columns={'time': 'date'})
    elif 'date' not in df.columns and 'block_time' in df.columns:
        df = df.rename(columns={'block_time': 'date'})

    if 'date' in df.columns:
        df['date'] = pd.to_datetime(df['date'])
        df = df.sort_values('date')

    # Si les colonnes des bandes ont des noms différents, renomme ici :
    # Exemple : df = df.rename(columns={'<1 day': '0d-1d', ...})

    return df

def get_fallback_data():
    """Fallback : Données simulées"""
    st.info("Utilisation de données de simulation pour l'affichage.")
    return generate_mock_data()

def generate_mock_data():
    """Génère des données fictives réalistes"""
    dates = pd.date_range(start='2020-01-01', end=datetime.now(), freq='D')
    df = pd.DataFrame({'date': dates})

    bands = [
        '0d-1d', '1d-1w', '1w-1m', '1m-3m', '3m-6m', '6m-12m',
        '12m-18m', '18m-2y', '2y-3y', '3y-5y', '5y-7y', '7y-10y', '10y+'
    ]

    n = len(dates)
    data = np.random.rand(n, len(bands))
    data = data / data.sum(axis=1)[:, None] * 100
    for i, band in enumerate(bands):
        df[band] = data[:, i]

    return df

def plot_realized_cap_utxo_age_bands(df: pd.DataFrame):
    """Graphique stacked area fidèle à l'original"""
    fig = go.Figure()

    colors = {
        '0d-1d': '#666666', '1d-1w': '#808080', '1w-1m': '#9c6644',
        '1m-3m': '#c38c5f', '3m-6m': '#e6b87d', '6m-12m': '#c4458f',
        '12m-18m': '#6b4fa0', '18m-2y': '#4a9c8f', '2y-3y': '#3d8f6e',
        '3y-5y': '#2e7c5e', '5y-7y': '#1f694e', '7y-10y': '#0f5a3f',
        '10y+': '#004d33'
    }

    bands_order = [
        '10y+', '7y-10y', '5y-7y', '3y-5y', '2y-3y', '18m-2y',
        '12m-18m', '6m-12m', '3m-6m', '1m-3m', '1w-1m', '1d-1w', '0d-1d'
    ]

    available_bands = [b for b in bands_order if b in df.columns]

    for col in available_bands:
        fig.add_trace(go.Scatter(
            x=df['date'],
            y=df[col],
            name=col,
            stackgroup='one',
            fillcolor=colors.get(col, '#888888'),
            line=dict(width=0.5, color='rgba(255,255,255,0.2)'),
            mode='lines'
        ))

    fig.update_layout(
        title="Bitcoin: Realized Cap - UTXO Age Bands (%)",
        height=750,
        template="plotly_dark",
        hovermode="x unified",
        legend=dict(orientation="h", y=-0.15, x=0.5, xanchor="center"),
        yaxis=dict(title="Pourcentage du Realized Cap (%)", range=[0, 100], ticksuffix="%"),
        xaxis=dict(title="Date"),
        margin=dict(t=50, b=100)
    )
    return fig

def get_realized_cap_utxo_plot():
    """Fonction principale appelée par Streamlit"""
    df = get_dune_utxo_realized_cap()
    if df is not None and not df.empty:
        fig = plot_realized_cap_utxo_age_bands(df)
        return fig
    return None

# ===================== TEST =====================
if __name__ == "__main__":
    df = generate_mock_data()
    fig = plot_realized_cap_utxo_age_bands(df)
    fig.show()
