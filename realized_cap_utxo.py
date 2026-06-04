import pandas as pd
import plotly.graph_objects as go
import requests
import streamlit as st
from datetime import datetime, timedelta
import numpy as np
from config_manager import get_cryptoquant_api_key

@st.cache_data(ttl=3600)
def get_cryptoquant_utxo_realized_cap():
    """Récupère les données dynamiques depuis CryptoQuant"""
    url = "https://api.cryptoquant.com/v1/btc/chart/network-indicator/realized-cap-utxo-age-bands-percent"

    api_key = get_cryptoquant_api_key()

    headers = {
        "accept": "application/json",
    }

    if not api_key:
        st.warning("Clé API CryptoQuant manquante dans la configuration. [cryptoquant.com](https://cryptoquant.com)")
        return get_fallback_data()

    headers["Authorization"] = f"Bearer {api_key}"

    try:
        response = requests.get(url, headers=headers, timeout=20)
        if response.status_code == 401:
            st.error("Erreur API CryptoQuant : 401 (Non autorisé). Veuillez vérifier votre clé API dans l'onglet Accueil.")
            return get_fallback_data()
        elif response.status_code != 200:
            st.error(f"Erreur API CryptoQuant ({response.status_code}): {response.text}")
            return get_fallback_data()

        data = response.json()
        if 'result' not in data or 'data' not in data['result']:
            st.error("Format de réponse CryptoQuant inattendu.")
            return get_fallback_data()

        df = pd.DataFrame(data['result']['data'])
        df['date'] = pd.to_datetime(df['date'])
        df = df.sort_values('date')

        return df
    except Exception as e:
        st.error(f"Erreur lors de la récupération des données CryptoQuant : {e}")
        return get_fallback_data()


def get_fallback_data():
    """Fallback : Données simulées si l'API échoue"""
    st.info("Utilisation de données de simulation pour l'affichage.")
    return generate_mock_data()


def generate_mock_data():
    """Génère des données fictives pour la démonstration"""
    dates = pd.date_range(start='2020-01-01', end=datetime.now(), freq='D')
    df = pd.DataFrame({'date': dates})

    bands = [
        '0d-1d', '1d-1w', '1w-1m', '1m-3m', '3m-6m', '6m-12m',
        '12m-18m', '18m-2y', '2y-3y', '3y-5y', '5y-7y', '7y-10y', '10y+'
    ]

    # Simulation de pourcentages qui s'additionnent à 100
    n = len(dates)
    data = np.random.rand(n, len(bands))
    data = data / data.sum(axis=1)[:, None] * 100

    for i, band in enumerate(bands):
        df[band] = data[:, i]

    return df


def plot_realized_cap_utxo_age_bands(df: pd.DataFrame):
    """Graphique fidèle à l'original"""

    fig = go.Figure()

    # Couleurs approximatives de l'original
    colors = {
        '0d-1d': '#666666', '1d-1w': '#808080', '1w-1m': '#9c6644',
        '1m-3m': '#c38c5f', '3m-6m': '#e6b87d', '6m-12m': '#c4458f',
        '12m-18m': '#6b4fa0', '18m-2y': '#4a9c8f', '2y-3y': '#3d8f6e',
        '3y-5y': '#2e7c5e', '5y-7y': '#1f694e', '7y-10y': '#0f5a3f',
        '10y+': '#004d33'
    }

    # On trie les colonnes pour avoir l'ordre logique dans le stack
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
    """Fonction principale pour Streamlit"""
    # st.subheader("📊 Bitcoin: Realized Cap - UTXO Age Bands (%)")
    # st.caption("Données dynamiques • Source : CryptoQuant")

    df = get_cryptoquant_utxo_realized_cap()

    if df is not None and not df.empty:
        fig = plot_realized_cap_utxo_age_bands(df)
        return fig
    else:
        return None

if __name__ == "__main__":
    # Test simple
    df = generate_mock_data()
    fig = plot_realized_cap_utxo_age_bands(df)
    fig.show()
