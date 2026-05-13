import pandas as pd
import yfinance as yf
import plotly.graph_objects as go
from plotly.subplots import make_subplots
import warnings
import streamlit as st

warnings.filterwarnings("ignore")

@st.cache_data(ttl=3600)
def get_btc_volume_plot():
    # ====================== Téléchargement des données ======================
    # Utilisation d'une période par défaut large pour le dashboard
    btc = yf.download('BTC-USD', start='2020-01-01', interval="1d", progress=False)

    if btc.empty:
        return None

    # Harmonisation MultiIndex yfinance si nécessaire
    if isinstance(btc.columns, pd.MultiIndex):
        btc_flat = pd.DataFrame(index=btc.index)
        for col in ['Open', 'High', 'Low', 'Close', 'Volume']:
            btc_flat[col] = btc[col]['BTC-USD']
        btc = btc_flat

    # Ajout de la colonne "Color" pour le volume
    btc['Color'] = btc.apply(lambda row: 'green' if row['Close'] >= row['Open'] else 'red', axis=1)

    # ====================== Graphique ======================
    fig = make_subplots(
        rows=2, cols=1,
        shared_xaxes=True,
        vertical_spacing=0.08,
        row_heights=[0.70, 0.30],
        subplot_titles=("BTC Price", "Volume")
    )

    # 1. Candlestick + ligne prix
    fig.add_trace(
        go.Candlestick(
            x=btc.index,
            open=btc['Open'],
            high=btc['High'],
            low=btc['Low'],
            close=btc['Close'],
            name="BTC Price",
            increasing_line_color='#00FF88',
            decreasing_line_color='#FF3333'
        ),
        row=1, col=1
    )

    fig.add_trace(
        go.Scatter(
            x=btc.index,
            y=btc['Close'],
            mode='lines',
            name='Close Price',
            line=dict(color='white', width=1.5),
            opacity=0.4
        ),
        row=1, col=1
    )

    # 2. Volume coloré
    fig.add_trace(
        go.Bar(
            x=btc.index,
            y=btc['Volume'],
            name='Volume',
            marker_color=btc['Color'],
            marker_line_color=btc['Color'],
            marker_line_width=0
        ),
        row=2, col=1
    )

    fig.update_layout(
        title="Bitcoin - Price + Volume (jours positifs/negatifs)",
        xaxis_title="Date",
        template="plotly_dark",
        height=850,
        hovermode="x unified",
        legend=dict(x=0.01, y=0.99),
        xaxis_rangeslider_visible=False
    )

    # Personnalisation des axes
    fig.update_yaxes(title_text="BTC Price (USD)", row=1, col=1)
    fig.update_yaxes(title_text="Volume", row=2, col=1)

    return fig

if __name__ == "__main__":
    fig = get_btc_volume_plot()
    if fig:
        fig.show()
