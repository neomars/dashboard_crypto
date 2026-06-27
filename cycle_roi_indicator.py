import pandas as pd
import yfinance as yf
import requests
import plotly.graph_objects as go
import streamlit as st
from datetime import datetime, timedelta

from io import StringIO

@st.cache_data(ttl=86400)
def fetch_cycle_roi_data():
    """
    Récupère les données historiques combinées du Bitcoin (GitHub + Yahoo Finance).
    """
    # 1. Données anciennes (2010-2018) via GitHub
    url = "https://raw.githubusercontent.com/Yrzxiong/Bitcoin-Dataset/master/bitcoin_dataset.csv"
    try:
        response = requests.get(url, timeout=10)
        btc_old = pd.read_csv(StringIO(response.text))
        btc_old['date'] = pd.to_datetime(btc_old['Date']).dt.tz_localize(None)
        btc_old['close'] = pd.to_numeric(btc_old['btc_market_price'], errors='coerce')
        btc_old = btc_old[['date', 'close']].dropna().sort_values('date').reset_index(drop=True)
    except Exception as e:
        btc_old = pd.DataFrame(columns=['date', 'close'])

    # 2. Données récentes via Yahoo Finance
    try:
        btc_new = yf.download('BTC-USD', start='2018-01-01', interval='1d', progress=False)
        if not btc_new.empty:
            if isinstance(btc_new.columns, pd.MultiIndex):
                btc_new = pd.DataFrame({'close': btc_new['Close']['BTC-USD']})
            else:
                btc_new = btc_new[['Close']]
                btc_new.columns = ['close']
            btc_new = btc_new.reset_index()
            btc_new.columns = ['date', 'close']
            btc_new['date'] = pd.to_datetime(btc_new['date']).dt.tz_localize(None)
        else:
            btc_new = pd.DataFrame(columns=['date', 'close'])
    except:
        btc_new = pd.DataFrame(columns=['date', 'close'])

    # 3. Fusion
    btc = pd.concat([btc_old, btc_new], ignore_index=True)
    btc = btc.drop_duplicates(subset='date').sort_values('date').reset_index(drop=True)
    btc = btc[btc['close'] > 0].reset_index(drop=True)

    return btc

def get_cycle_tops(btc_df):
    """
    Identifie les Tops de cycles en utilisant la même logique que btc_halving.py
    """
    known_halvings = [
        {"date": "2012-11-28"},
        {"date": "2016-07-09"},
        {"date": "2020-05-11"},
        {"date": "2024-04-20"}
    ]
    halving_dates = [pd.to_datetime(h["date"]) for h in known_halvings]

    # Prochain halving estimé
    try:
        current_block = int(requests.get("https://mempool.space/api/blocks/tip/height", timeout=10).text.strip())
    except:
        current_block = 840000
    block_interval = 210000
    next_halving_block = ((current_block // block_interval) + 1) * block_interval
    blocks_to_next = next_halving_block - current_block
    estimated_date = datetime.now() + timedelta(days=(blocks_to_next * 10) / (60 * 24))

    all_halvings = halving_dates + [pd.to_datetime(estimated_date)]

    tops = []
    for i in range(len(all_halvings) - 1):
        start_date = all_halvings[i]
        next_halving_date = all_halvings[i+1]
        top_end_limit = next_halving_date - timedelta(days=150)

        period_data = btc_df[(btc_df['date'] >= start_date) & (btc_df['date'] < top_end_limit)]
        if not period_data.empty:
            top_row = period_data.loc[period_data['close'].idxmax()]
            tops.append({
                "name": f"Cycle {top_row['date'].year}",
                "date": top_row['date']
            })
    return tops

def calculate_cycle_rois(df: pd.DataFrame):
    """
    Calcule le ROI depuis chaque top de cycle.
    """
    if df is None or df.empty:
        return []

    df['date'] = pd.to_datetime(df['date']).dt.tz_localize(None)
    cycle_tops = get_cycle_tops(df)

    results = []
    for cycle in cycle_tops:
        top_date = cycle['date']
        cycle_df = df[df['date'] >= top_date].copy()

        if cycle_df.empty:
            continue

        top_price = cycle_df.iloc[0]['close']
        cycle_df['days_since_top'] = (cycle_df['date'] - top_date).dt.days
        # Utilisation du multiplicateur pour compatibilité avec l'échelle logarithmique
        cycle_df['roi'] = (cycle_df['close'] / top_price)
        cycle_df['cycle_name'] = cycle['name']

        results.append(cycle_df[['days_since_top', 'roi', 'cycle_name', 'date']])

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
            x=cycle_df['days_since_top'],
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
        title="Bitcoin Market Cycle ROI (Performance depuis le Top)",
        xaxis_title="Jours depuis le top du cycle",
        yaxis_title="ROI (Multiplicateur)",
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
