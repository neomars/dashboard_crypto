import pandas as pd
import yfinance as yf
import plotly.graph_objects as go
from plotly.subplots import make_subplots
import streamlit as st

@st.cache_data(ttl=3600)
def fetch_bmsb_data():
    """
    Récupère les données hebdomadaires du Bitcoin via Yahoo Finance.
    """
    # On commence en 2010 pour avoir l'historique complet
    btc = yf.download('BTC-USD', start='2010-01-01', interval='1wk', progress=False)
    if btc.empty:
        return None

    # Nettoyage des colonnes (yfinance peut renvoyer un MultiIndex)
    if isinstance(btc.columns, pd.MultiIndex):
        btc.columns = btc.columns.get_level_values(0)

    # Pour les versions récentes de yfinance qui renvoient des noms de colonnes bizarres avec 1 seul ticker
    if 'Close' not in btc.columns and any('Close' in str(c) for c in btc.columns):
        btc.columns = [c[0] if isinstance(c, tuple) else c for c in btc.columns]

    btc = btc.reset_index()
    btc.columns = [str(c).lower() for c in btc.columns]

    return btc

def calculate_bear_market_support_band(df: pd.DataFrame, sma_length: int = 20, ema_length: int = 21):
    """
    Calcule le Bear Market Support Band (Benjamin Cowen)
    - 20-week SMA (vert)
    - 21-week EMA (rouge)
    """
    df = df.copy()
    df['date'] = pd.to_datetime(df['date'])
    df = df.sort_values('date')

    # Calcul des bandes
    df['bmsb_sma20'] = df['close'].rolling(window=sma_length, min_periods=1).mean()
    df['bmsb_ema21'] = df['close'].ewm(span=ema_length, adjust=False).mean()

    # Détermination du régime de marché
    df['market_regime'] = df.apply(
        lambda x: "Bull Market" if x['close'] > x['bmsb_sma20'] else "Bear Market", axis=1
    )

    return df

def plot_bear_market_support_band(df: pd.DataFrame, sma_len: int, ema_len: int):
    """
    Retourne un graphique Plotly du Bear Market Support Band
    """
    fig = make_subplots(rows=1, cols=1, shared_xaxes=True,
                        vertical_spacing=0.02)

    # Prix en bougies si disponible
    if all(col in df.columns for col in ['open', 'high', 'low', 'close']):
        fig.add_trace(go.Candlestick(
            x=df['date'],
            open=df['open'],
            high=df['high'],
            low=df['low'],
            close=df['close'],
            name="BTC Price",
            increasing_line_color='#00ff88',
            decreasing_line_color='#ff3366'
        ), row=1, col=1)
    else:
        fig.add_trace(go.Scatter(
            x=df['date'], y=df['close'],
            name="BTC Price",
            line=dict(color='#00CCFF', width=2)
        ), row=1, col=1)

    # Bandes
    fig.add_trace(go.Scatter(
        x=df['date'], y=df['bmsb_sma20'],
        name=f"{sma_len}-week SMA (Support)",
        line=dict(color='#00ff00', width=2.5)
    ), row=1, col=1)

    fig.add_trace(go.Scatter(
        x=df['date'], y=df['bmsb_ema21'],
        name=f"{ema_len}-week EMA (Resistance)",
        line=dict(color='#ff0000', width=2.5)
    ), row=1, col=1)

    # Remplissage entre les bandes
    fig.add_trace(go.Scatter(
        x=df['date'], y=df['bmsb_sma20'],
        fill=None,
        mode='lines',
        line=dict(color='rgba(0,0,0,0)'),
        showlegend=False
    ), row=1, col=1)

    fig.add_trace(go.Scatter(
        x=df['date'], y=df['bmsb_ema21'],
        fill='tonexty',
        mode='lines',
        line=dict(color='rgba(0,0,0,0)'),
        fillcolor='rgba(255, 100, 100, 0.15)',
        name="Zone Bear Market"
    ), row=1, col=1)

    fig.update_layout(
        height=700,
        template="plotly_dark",
        hovermode="x unified",
        legend=dict(orientation="h", yanchor="bottom", y=1.02, xanchor="right", x=1),
        xaxis_title="Date",
        yaxis_title="Prix BTC (USD)",
        margin=dict(t=50)
    )

    return fig

def get_bmsb_plot():
    """Fonction principale à appeler dans Streamlit"""
    col1, col2 = st.columns([3, 1])
    with col2:
        sma_len = st.slider("Longueur SMA (Semaines)", 10, 50, 20)
        ema_len = st.slider("Longueur EMA (Semaines)", 10, 50, 21)

    df = fetch_bmsb_data()
    if df is None or df.empty:
        st.error("Impossible de récupérer les données Yahoo Finance.")
        return None

    df_calc = calculate_bear_market_support_band(df, sma_length=sma_len, ema_length=ema_len)

    # Affichage du régime actuel
    current_regime = df_calc.iloc[-1]['market_regime']
    color = "🟢" if current_regime == "Bull Market" else "🔴"
    with col1:
        st.metric("Régime actuel", f"{color} {current_regime}")

    fig = plot_bear_market_support_band(df_calc, sma_len, ema_len)
    return fig

if __name__ == "__main__":
    # Test simple hors Streamlit
    print("Test du BMSB...")
    df = fetch_bmsb_data()
    if df is not None:
        df_calc = calculate_bear_market_support_band(df)
        print(df_calc.tail())
    else:
        print("Erreur de récupération des données")
