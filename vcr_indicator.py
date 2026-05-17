import pandas as pd
import yfinance as yf
import numpy as np
import plotly.graph_objects as go
from plotly.subplots import make_subplots
import streamlit as st
from datetime import datetime, timedelta

@st.cache_data(ttl=3600)
def fetch_vcr_data():
    btc = yf.download('BTC-USD', start='2018-01-01', interval='1d', progress=False)
    if btc.empty:
        return None

    if isinstance(btc.columns, pd.MultiIndex):
        btc = pd.DataFrame({'Close': btc['Close']['BTC-USD']})
    else:
        btc = btc[['Close']]

    btc['Return'] = np.log(btc['Close'] / btc['Close'].shift(1))
    return btc

def vol(series, window):
    return series.rolling(window).std() * np.sqrt(365) * 100

def get_vcr_plot():
    btc = fetch_vcr_data()
    if btc is None:
        return None

    # Volatilités & VCR
    btc['Vol_30']  = vol(btc['Return'], 30)
    btc['Vol_365'] = vol(btc['Return'], 365)
    btc['VCR_30_365'] = btc['Vol_30'] / btc['Vol_365']

    # ====================== Graphique interactif ======================
    fig = make_subplots(
        rows=2, cols=1,
        shared_xaxes=True,
        vertical_spacing=0.1,
        row_heights=[0.65, 0.35]
    )

    # BTC Price
    fig.add_trace(go.Scatter(
        x=btc.index, y=btc['Close'],
        mode='lines', name='Prix BTC',
        line=dict(color='white', width=2)
    ), row=1, col=1)

    # VCR principal (30j / 365j)
    fig.add_trace(go.Scatter(
        x=btc.index, y=btc['VCR_30_365'],
        mode='lines', name='VCR 30/365',
        line=dict(color='#00FFAA', width=3)
    ), row=2, col=1)

    # Zones de probabilité
    fig.add_hrect(y0=0, y1=0.55, fillcolor="red", opacity=0.15, line_width=0, row=2, col=1,
                  annotation_text="Très forte probabilité de fort mouvement", annotation_position="top left",
                  annotation_font=dict(size=10, color="red"))
    fig.add_hrect(y0=0.55, y1=0.75, fillcolor="orange", opacity=0.15, line_width=0, row=2, col=1,
                  annotation_text="Forte probabilité d'expansion", annotation_position="top left",
                  annotation_font=dict(size=10, color="orange"))

    # Lignes de référence
    fig.add_hline(y=1.0, line_dash="dash", line_color="gray", row=2, col=1)
    fig.add_hline(y=0.6, line_dash="dot", line_color="red", row=2, col=1)

    # Boutons de période dynamiques
    now = datetime.now()
    one_year_ago = (now - timedelta(days=365)).strftime('%Y-%m-%d')
    two_years_ago = (now - timedelta(days=730)).strftime('%Y-%m-%d')
    three_years_ago = (now - timedelta(days=1095)).strftime('%Y-%m-%d')
    end_date = (now + timedelta(days=30)).strftime('%Y-%m-%d')

    fig.update_layout(
        xaxis_title="Date",
        yaxis_title="BTC Price (USD)",
        yaxis2_title="VCR (30j / 365j)",
        template="plotly_dark",
        height=800,
        hovermode="x unified",
        legend=dict(orientation="h", yanchor="bottom", y=1.02, xanchor="right", x=1),
        margin=dict(t=150, b=50),
        updatemenus=[dict(
            type="buttons",
            direction="right",
            x=0.0,
            y=1.2,
            showactive=True,
            buttons=[
                dict(label="Tout", method="relayout", args=[{"xaxis.autorange": True}]),
                dict(label="3 ans", method="relayout", args=[{"xaxis.range": [three_years_ago, end_date]}]),
                dict(label="2 ans", method="relayout", args=[{"xaxis.range": [two_years_ago, end_date]}]),
                dict(label="1 an",  method="relayout", args=[{"xaxis.range": [one_year_ago, end_date]}]),
            ]
        )]
    )

    fig.update_yaxes(type="log", row=1, col=1)

    return fig
