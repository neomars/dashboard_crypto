import pandas as pd
import numpy as np
import yfinance as yf
import plotly.graph_objects as go
from datetime import datetime, timedelta

def run_simulation(start_date, end_date, drop_threshold_pct, target_leverage=2.0):
    # 1. Download Data
    df = yf.download('BTC-USD', start=start_date, end=end_date, interval='1d', progress=False)
    if df.empty:
        return None, None

    if isinstance(df.columns, pd.MultiIndex):
        df = pd.DataFrame({'Close': df['Close']['BTC-USD']})
    else:
        df = df[['Close']]

    df = df.sort_index()

    # 2. Simulation variables
    initial_capital = 10000.0
    cash = 0.0
    btc_units = initial_capital / df.iloc[0]['Close']

    equity = [initial_capital]
    states = [] # 'Normal', 'Closing'

    current_state = 'Normal'
    entry_price = df.iloc[0]['Close']
    leverage = 1.0

    closing_start_date = None
    initial_position_to_close = 0
    weeks_closed = 0

    # Track performance
    history = []

    for i in range(len(df)):
        current_price = df.iloc[i]['Close']
        current_date = df.index[i]

        # Calculate current equity
        # Equity = Cash + (BTC_units * Price) - Debt
        # Debt exists if leverage > 1.
        # Initial Debt = Position_Value * (Leverage - 1) / Leverage ?
        # Actually, simpler:
        # If we are x1: Equity = btc_units * current_price
        # If we switch to x2: we buy more BTC using debt.
        # New_btc_units = old_btc_units * 2.
        # Debt = old_btc_units * entry_price_at_switch

        if current_state == 'Normal':
            # Check for drop
            price_drop = (current_price - entry_price) / entry_price
            if price_drop <= -drop_threshold_pct / 100.0:
                current_state = 'Closing'
                closing_start_date = current_date
                # Double the position using debt
                # New Equity stays the same at the moment of switch
                # Position becomes 2x Equity
                current_equity = btc_units * current_price
                btc_units = (current_equity * target_leverage) / current_price
                debt = current_equity * (target_leverage - 1.0)
                initial_position_to_close = btc_units
                weeks_closed = 0

            current_equity = btc_units * current_price

        elif current_state == 'Closing':
            # Every 7 days, close 1/10th
            days_since_start = (current_date - closing_start_date).days
            expected_weeks_passed = days_since_start // 7

            if expected_weeks_passed > weeks_closed:
                # Close 1/10th of the INITIAL position that was opened at switch
                units_to_close = initial_position_to_close / 10.0
                # Reduce units, increase cash (pay debt first)
                cash_from_sale = units_to_close * current_price
                btc_units -= units_to_close

                # Pay debt first
                if debt > 0:
                    payment = min(cash_from_sale, debt)
                    debt -= payment
                    cash_from_sale -= payment

                cash += cash_from_sale
                weeks_closed += 1

                if weeks_closed >= 10 or btc_units <= 0:
                    # Simulation says: "rachète du btc"
                    total_equity = cash + (btc_units * current_price) - debt
                    btc_units = total_equity / current_price
                    cash = 0
                    debt = 0
                    current_state = 'Normal'
                    entry_price = current_price

            current_equity = cash + (btc_units * current_price) - debt

        history.append({
            'Date': current_date,
            'BTC_Price': current_price,
            'Equity': current_equity,
            'State': current_state
        })

    history_df = pd.DataFrame(history)
    history_df['BuyHold'] = initial_capital * (history_df['BTC_Price'] / history_df['BTC_Price'].iloc[0])

    return history_df, df.index

def get_simulator_plot(history_df):
    if history_df is None:
        return None

    fig = go.Figure()

    # BTC Price (Right Axis)
    fig.add_trace(go.Scatter(
        x=history_df['Date'],
        y=history_df['BTC_Price'],
        name="Prix BTC",
        line=dict(color='rgba(255, 255, 255, 0.3)', width=1),
        yaxis="y2"
    ))

    # Strategy Equity
    fig.add_trace(go.Scatter(
        x=history_df['Date'],
        y=history_df['Equity'],
        name="Stratégie Simulator",
        line=dict(color='#00FFAA', width=2)
    ))

    # Buy & Hold
    fig.add_trace(go.Scatter(
        x=history_df['Date'],
        y=history_df['BuyHold'],
        name="Buy & Hold BTC",
        line=dict(color='orange', width=1.5, dash='dash')
    ))

    # Halvings (Vertical lines)
    halvings = [
        datetime(2012, 11, 28),
        datetime(2016, 7, 9),
        datetime(2020, 5, 11),
        datetime(2024, 4, 20)
    ]

    for h in halvings:
        if history_df['Date'].min() <= h <= history_df['Date'].max():
            fig.add_vline(x=h, line_dash="dot", line_color="red", annotation_text="Halving")

    fig.update_layout(
        title="Simulateur d'Investissement Bitcoin",
        xaxis_title="Date",
        yaxis=dict(title="Valeur du Capital (USD)", type="log"),
        yaxis2=dict(title="Prix BTC (USD)", overlaying='y', side='right', type="log"),
        template="plotly_dark",
        height=700,
        hovermode="x unified"
    )

    return fig
