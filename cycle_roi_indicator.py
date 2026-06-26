import pandas as pd
import yfinance as yf
import plotly.graph_objects as go
import streamlit as st
from datetime import datetime

# Liste des bottoms de cycle (basée sur la demande de l'utilisateur)
CYCLE_BOTTOMS = [
    {"name": "Cycle 2015", "date": "2015-01-14"},
    {"name": "Cycle 2018", "date": "2018-12-15"},
    {"name": "Cycle 2020 (Covid)", "date": "2020-03-12"},
    {"name": "Cycle 2022", "date": "2022-11-21"},
]

@st.cache_data(ttl=3600)
def fetch_cycle_roi_data():
    """
    Récupère les données historiques du Bitcoin via Yahoo Finance.
    """
    # On commence en 2014 pour couvrir les cycles demandés
    df = yf.download('BTC-USD', start='2014-01-01', interval='1d', progress=False)
    if df.empty:
        return None

    if isinstance(df.columns, pd.MultiIndex):
        df.columns = df.columns.get_level_values(0)

    df = df.reset_index()
    df.columns = [str(c).lower() for c in df.columns]

    if 'date' not in df.columns and 'index' in df.columns:
        df = df.rename(columns={'index': 'date'})

    return df

def calculate_cycle_rois(df: pd.DataFrame):
    """
    Calcule le ROI depuis chaque bottom de cycle.
    ROI = (Prix / Prix au Bottom) - 1
    """
    if df is None or df.empty:
        return []

    results = []
    df['date'] = pd.to_datetime(df['date']).dt.tz_localize(None)

    for cycle in CYCLE_BOTTOMS:
        bottom_date = pd.to_datetime(cycle['date'])
        cycle_df = df[df['date'] >= bottom_date].copy()

        if cycle_df.empty:
            continue

        bottom_price = cycle_df.iloc[0]['close']
        cycle_df['days_since_bottom'] = (cycle_df['date'] - bottom_date).dt.days
        # Formule demandée par l'utilisateur : (price / bottom) - 1
        cycle_df['roi'] = (cycle_df['close'] / bottom_price) - 1
        cycle_df['cycle_name'] = cycle['name']

        results.append(cycle_df[['days_since_bottom', 'roi', 'cycle_name', 'date']])

    return results

def get_cycle_roi_plot():
    """
    Fonction principale pour Streamlit.
    """
    df = fetch_cycle_roi_data()
    if df is None:
        st.error("Impossible de récupérer les données Yahoo Finance.")
        return None

    cycle_data_list = calculate_cycle_rois(df)

    fig = go.Figure()

    # Couleurs
    colors = ['#00FF00', '#00BFFF', '#FFD700', '#FF00FF', '#FFFFFF']

    for i, cycle_df in enumerate(cycle_data_list):
        fig.add_trace(go.Scatter(
            x=cycle_df['days_since_bottom'],
            y=cycle_df['roi'],
            mode='lines',
            name=cycle_df['cycle_name'].iloc[0],
            line=dict(width=2.5, color=colors[i % len(colors)]),
            hovertemplate="<b>%{fullData.name}</b><br>" +
                          "Jours: %{x}<br>" +
                          "ROI: %{y:.2f}x<br>" +
                          "Date: %{customdata}<extra></extra>",
            customdata=cycle_df['date'].dt.strftime('%Y-%m-%d')
        ))

    fig.update_layout(
        title="Bitcoin Market Cycle ROI (Performance depuis le Bottom)",
        xaxis_title="Jours depuis le bottom du cycle",
        yaxis_title="ROI (Multiplicateur - 1)",
        template="plotly_dark",
        height=700,
        hovermode="x unified",
        legend=dict(orientation="h", yanchor="bottom", y=1.02, xanchor="right", x=1),
        margin=dict(t=100, l=80)
    )

    return fig

if __name__ == "__main__":
    # Test
    fig = get_cycle_roi_plot()
    if fig:
        fig.show()
