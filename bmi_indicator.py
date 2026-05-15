import pandas as pd
import yfinance as yf
import numpy as np
import plotly.graph_objects as go
from plotly.subplots import make_subplots
import streamlit as st
from datetime import datetime

@st.cache_data(ttl=3600)
def fetch_bmi_data():
    # On prend un historique large pour le calcul du BMI
    btc = yf.download('BTC-USD', start='2015-01-01', interval='1d', progress=False)
    if btc.empty:
        return None

    if isinstance(btc.columns, pd.MultiIndex):
        btc = pd.DataFrame({'close': btc['Close']['BTC-USD']})
    else:
        btc = btc[['Close']]
        btc.columns = ['close']

    btc = btc.reset_index()
    btc.columns = ['date', 'close']
    return btc

def find_corrections(df, min_drop=-15):
    corrections = []
    if df.empty:
        return corrections
    peak = df['close'].iloc[0]
    for i in range(1, len(df)):
        if df['close'].iloc[i] > peak:
            peak = df['close'].iloc[i]
        else:
            drop = (df['close'].iloc[i] / peak - 1) * 100
            if drop <= min_drop:
                corrections.append(drop)
    return corrections

def get_bmi_plot():
    btc = fetch_bmi_data()
    if btc is None:
        return None

    # Calcul des corrections historiques
    all_corrections = find_corrections(btc, min_drop=-15)
    historical_avg_correction = np.abs(all_corrections).mean() if all_corrections else 40

    # BMI (24 mois rolling)
    window = 730  # ~24 mois
    btc['BMI'] = np.nan

    # Optimisation du calcul rolling pour Streamlit (on ne recalcule pas tout si possible,
    # mais ici on suit la logique utilisateur)
    # Pour accélérer, on peut limiter le calcul aux points visibles ou utiliser des vecteurs si possible.
    # La logique find_corrections est itérative, donc on garde la boucle.

    # On calcule le BMI par paliers pour ne pas geler l'interface si trop de points
    # Ou on utilise une approche plus vectorisée pour le peak.

    closes = btc['close'].values
    bmi_values = np.full(len(btc), np.nan)

    for i in range(window, len(btc)):
        # Fenêtre de 24 mois
        window_closes = closes[i-window:i]

        # Logique simplifiée de find_corrections pour la performance
        peak = window_closes[0]
        recent_corrections = []
        for val in window_closes[1:]:
            if val > peak:
                peak = val
            else:
                drop = (val / peak - 1) * 100
                if drop <= -15:
                    recent_corrections.append(drop)

        if recent_corrections:
            recent_avg = np.abs(recent_corrections).mean()
            bmi_values[i] = recent_avg / historical_avg_correction

    btc['BMI'] = bmi_values

    # Estimation de la correction maximale
    btc['Max_Correction_Est'] = - (80 / (btc['BMI'] + 0.3))

    # ====================== Graphique ======================
    fig = make_subplots(
        rows=2, cols=1,
        shared_xaxes=True,
        vertical_spacing=0.08,
        row_heights=[0.65, 0.35],
        subplot_titles=("Prix BTC + Halvings", "Bitcoin Maturation Index (BMI)")
    )

    # Prix BTC
    fig.add_trace(go.Scatter(
        x=btc['date'], y=btc['close'],
        mode='lines', name='Prix BTC',
        line=dict(color='white', width=2)
    ), row=1, col=1)

    # BMI
    fig.add_trace(go.Scatter(
        x=btc['date'], y=btc['BMI'],
        mode='lines', name='BMI',
        line=dict(color='#00FFAA', width=3),
        customdata=btc['Max_Correction_Est'],
        hovertemplate="<b>%{x|%Y-%m-%d}</b><br>BMI: %{y:.2f}<br>Correction max estimée : %{customdata:.1f}%<extra></extra>"
    ), row=2, col=1)

    # Halvings
    halvings = {
        '2012-11-28': '1er Halving',
        '2016-07-09': '2ème Halving',
        '2020-05-11': '3ème Halving',
        '2024-04-20': '4ème Halving'
    }

    for date_str, label in halvings.items():
        dt = pd.to_datetime(date_str)
        if dt >= btc['date'].min():
            fig.add_vline(x=dt, line=dict(color="red", width=2, dash="dash"), row=1, col=1)
            fig.add_annotation(
                x=dt, y=btc['close'].max()*0.9,
                text=label, showarrow=True, arrowhead=2,
                arrowcolor="red", font=dict(color="white", size=10),
                bgcolor="rgba(200,0,0,0.8)", row=1, col=1
            )

    fig.update_layout(
        xaxis_title="Date",
        yaxis_title="BTC Price (USD)",
        yaxis2_title="BMI (bas = mature)",
        template="plotly_dark",
        height=850,
        hovermode="x unified",
        margin=dict(t=100)
    )

    fig.update_yaxes(type="log", row=1, col=1)

    return fig
