import pandas as pd
import plotly.graph_objects as go
import streamlit as st
from datetime import timedelta

from data_provider import KNOWN_HALVINGS, estimate_next_halving, get_combined_btc_history

def get_cycle_tops(btc_df):
    """
    Identifie les Tops de cycles en utilisant la même logique que btc_halving.py
    """
    halving_dates = [pd.to_datetime(h["date"]) for h in KNOWN_HALVINGS]
    estimated_date = estimate_next_halving()
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
    df = get_combined_btc_history()
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
