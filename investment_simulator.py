import pandas as pd
import numpy as np
import yfinance as yf
import plotly.graph_objects as go
from datetime import datetime, timedelta

def run_simulation(start_date, end_date, initial_investment, drop_threshold_pct, target_leverage=2.0, exit_frequency='Hebdomadaire', exit_pct=10.0):
    # 1. Download Data (with some buffer to detect peak before start if needed, but here we start at x1)
    df = yf.download('BTC-USD', start=start_date, end=end_date, interval='1d', progress=False)
    if df.empty:
        return None

    if isinstance(df.columns, pd.MultiIndex):
        df = pd.DataFrame({'Close': df['Close']['BTC-USD']})
    else:
        df = df[['Close']]

    df = df.sort_index()

    # 2. Simulation variables
    portfolio_value = float(initial_investment)
    btc_price_start = df.iloc[0]['Close']

    # State
    # current_mode: 'X1' or 'XL'
    current_mode = 'X1'
    all_time_high = btc_price_start
    waiting_for_recovery = False

    # Position tracking
    # Initial purchase is X1
    btc_units = portfolio_value / btc_price_start
    debt = 0.0

    trades = [{
        'Date': df.index[0],
        'Action': 'Achat Initial (Mode X1)',
        'Prix BTC': f'${btc_price_start:,.2f}',
        'Détails': f'Investissement de ${portfolio_value:,.2f} pour {btc_units:.4f} BTC'
    }]

    # Closing tracking
    closing_start_date = None
    initial_units_to_close = 0
    steps_passed = 0

    freq_map = {
        'Journalière': 1,
        'Hebdomadaire': 7,
        'Mensuelle': 30
    }
    days_per_step = freq_map.get(exit_frequency, 7)
    total_steps = int(100 / exit_pct)

    history = []
    liquidation_event = None

    # Portfolio ATH for drawdown calculation
    portfolio_ath = portfolio_value

    for i in range(len(df)):
        current_date = df.index[i]
        current_price = df.iloc[i]['Close']

        if current_mode == 'X1':
            # Update ATH de la séquence
            if current_price > all_time_high:
                all_time_high = current_price
                waiting_for_recovery = False # New ATH means we are no longer in a "drop" state relative to previous ATH

            # Current equity
            portfolio_value = btc_units * current_price

            # Check for drop from ALL TIME HIGH
            price_drop_from_ath = (current_price - all_time_high) / all_time_high

            # Condition de déclenchement : baisse de X% ET on n'est pas en train d'attendre une remontée
            if price_drop_from_ath <= -drop_threshold_pct / 100.0 and not waiting_for_recovery:
                # SWITCH TO XL
                current_mode = 'XL'
                closing_start_date = current_date
                steps_passed = 0

                old_units = btc_units
                # We want total position = portfolio_value * leverage
                # New btc units = (portfolio_value * leverage) / price
                new_btc_units = (portfolio_value * target_leverage) / current_price
                debt = (new_btc_units - btc_units) * current_price
                btc_units = new_btc_units
                initial_units_to_close = btc_units # The total amount to de-leverage

                trades.append({
                    'Date': current_date,
                    'Action': f'Passage en Levier x{target_leverage:.1f}',
                    'Prix BTC': f'${current_price:,.2f}',
                    'Détails': f'Achat de {new_btc_units - old_units:.4f} BTC via dette (${debt:,.2f})'
                })

        elif current_mode == 'XL':
            # Current equity
            portfolio_value = (btc_units * current_price) - debt

            # Progressive exit check
            days_since_switch = (current_date - closing_start_date).days
            expected_steps = days_since_switch // days_per_step

            if expected_steps > steps_passed:
                # Sell a fraction of initial XL units
                units_to_sell = initial_units_to_close * (exit_pct / 100.0)
                # If we have less than that (rounding), sell all
                units_to_sell = min(units_to_sell, btc_units)

                proceeds = units_to_sell * current_price
                btc_units -= units_to_sell

                # Use proceeds to pay debt first, then buy X1 BTC
                debt_paid = 0
                if debt > 0:
                    debt_paid = min(proceeds, debt)
                    debt -= debt_paid
                    proceeds -= debt_paid

                # If there's leftover proceeds (profit from that fraction), buy X1 BTC
                added_units = 0
                if proceeds > 0:
                    added_units = proceeds / current_price
                    btc_units += added_units

                trades.append({
                    'Date': current_date,
                    'Action': f'Sortie progressive ({exit_frequency} {expected_steps}/{total_steps})',
                    'Prix BTC': f'${current_price:,.2f}',
                    'Détails': f'Vente {units_to_sell:.4f} BTC. Dette payée: ${debt_paid:,.2f}. Réinvesti: {added_units:.4f} BTC'
                })

                steps_passed = expected_steps

                # If all steps passed, we are back to full X1 (debt should be 0 or small)
                if steps_passed >= total_steps:
                    current_mode = 'X1'
                    # On revient en X1. On active l'attente de récupération si on est toujours sous le seuil de drop
                    portfolio_value = (btc_units * current_price) - debt
                    btc_units = portfolio_value / current_price
                    debt = 0.0

                    trades.append({
                        'Date': current_date,
                        'Action': 'Fin de phase Levier',
                        'Prix BTC': f'${current_price:,.2f}',
                        'Détails': 'Retour au mode X1 (100% investi)'
                    })

                    # Si on est toujours en baisse de X% par rapport à l'ATH, on attend de repasser au dessus
                    # du seuil avant de pouvoir redéclencher (pour "perdre à nouveau X%")
                    price_drop_from_ath = (current_price - all_time_high) / all_time_high
                    if price_drop_from_ath <= -drop_threshold_pct / 100.0:
                        waiting_for_recovery = True

        # Check for liquidation
        if portfolio_value <= 0 and liquidation_event is None:
            liquidation_event = {
                'Date': current_date,
                'Portfolio_Value': 0
            }
            trades.append({
                'Date': current_date,
                'Action': '💀 LIQUIDATION 💀',
                'Prix BTC': f'${current_price:,.2f}',
                'Détails': 'Le capital net est tombé à zéro.'
            })
            portfolio_value = 0
            btc_units = 0
            debt = 0

        # Calculate Drawdown
        if portfolio_value > portfolio_ath:
            portfolio_ath = portfolio_value

        drawdown = (portfolio_value - portfolio_ath) / portfolio_ath * 100

        history.append({
            'Date': current_date,
            'BTC_Price': current_price,
            'Portfolio_Value': portfolio_value,
            'Mode': current_mode,
            'BTC_Units': btc_units,
            'Drawdown': drawdown
        })

    history_df = pd.DataFrame(history)
    # Buy & Hold for comparison
    history_df['Buy_Hold'] = initial_investment * (history_df['BTC_Price'] / btc_price_start)

    trades_df = pd.DataFrame(trades)

    return history_df, trades_df

def get_simulator_plot(history_df, trades_df):
    if history_df is None or history_df.empty:
        return None

    from plotly.subplots import make_subplots

    # Création d'un graphique avec deux lignes (Capital et Drawdown)
    fig = make_subplots(rows=2, cols=1, shared_xaxes=True,
                        vertical_spacing=0.05, row_heights=[0.8, 0.2])

    # Portfolio Value
    fig.add_trace(go.Scatter(
        x=history_df['Date'],
        y=history_df['Portfolio_Value'],
        name="Valeur Portefeuille (Stratégie)",
        line=dict(color='#00FFAA', width=2.5)
    ), row=1, col=1)

    # Buy & Hold
    fig.add_trace(go.Scatter(
        x=history_df['Date'],
        y=history_df['Buy_Hold'],
        name="Buy & Hold BTC",
        line=dict(color='rgba(255, 165, 0, 0.6)', width=1.5, dash='dash')
    ), row=1, col=1)

    # BTC Price (Right Axis - Row 1)
    fig.add_trace(go.Scatter(
        x=history_df['Date'],
        y=history_df['BTC_Price'],
        name="Prix BTC",
        line=dict(color='rgba(255, 255, 255, 0.1)', width=1),
        yaxis="y3"
    ), row=1, col=1)

    # Drawdown (Row 2)
    fig.add_trace(go.Scatter(
        x=history_df['Date'],
        y=history_df['Drawdown'],
        name="Drawdown (%)",
        line=dict(color='#FF5555', width=1),
        fill='tozeroy',
        fillcolor='rgba(255, 85, 85, 0.2)'
    ), row=2, col=1)

    # Vertical Lines for Halvings
    halvings = [
        datetime(2012, 11, 28),
        datetime(2016, 7, 9),
        datetime(2020, 5, 11),
        datetime(2024, 4, 20)
    ]

    for h in halvings:
        if history_df['Date'].min() <= h <= history_df['Date'].max():
            fig.add_shape(
                type="line", x0=h, x1=h, y0=0, y1=1,
                yref="paper", line=dict(color="rgba(255, 0, 0, 0.3)", width=1, dash="dot")
            )

    # Points d'entrée et sortie en Levier
    if not trades_df.empty:
        entries = trades_df[trades_df['Action'].str.contains('Passage en Levier')]
        exits = trades_df[trades_df['Action'].str.contains('Fin de phase Levier')]

        # On merge avec history_df pour avoir les valeurs de portefeuille aux dates précises
        merged_entries = pd.merge(entries, history_df, on='Date')
        merged_exits = pd.merge(exits, history_df, on='Date')

        fig.add_trace(go.Scatter(
            x=merged_entries['Date'],
            y=merged_entries['Portfolio_Value'],
            mode='markers',
            name='Entrée Levier',
            marker=dict(color='lime', size=12, symbol='triangle-up', line=dict(color='white', width=1)),
            hoverinfo='skip'
        ), row=1, col=1)

        fig.add_trace(go.Scatter(
            x=merged_exits['Date'],
            y=merged_exits['Portfolio_Value'],
            mode='markers',
            name='Sortie Levier',
            marker=dict(color='red', size=10, symbol='circle', line=dict(color='white', width=1)),
            hoverinfo='skip'
        ), row=1, col=1)

    # Marquage des liquidations
    liquidations = trades_df[trades_df['Action'].str.contains('LIQUIDATION')]
    if not liquidations.empty:
        merged_liq = pd.merge(liquidations, history_df, on='Date')
        fig.add_trace(go.Scatter(
            x=merged_liq['Date'],
            y=merged_liq['Portfolio_Value'],
            mode='markers+text',
            name='Liquidation',
            text='💀',
            textposition='top center',
            marker=dict(color='orange', size=15, symbol='x'),
            hoverinfo='skip'
        ), row=1, col=1)

    fig.update_layout(
        title="Simulation d'Investissement BTC - Stratégie de Levier Dynamique",
        xaxis2_title="Date",
        xaxis=dict(range=[history_df['Date'].min(), history_df['Date'].max()]),
        yaxis=dict(title="Capital (USD)"),
        yaxis2=dict(title="Drawdown (%)", side="left", showgrid=True, gridcolor="rgba(255,255,255,0.05)"),
        yaxis3=dict(title="Prix BTC (USD)", overlaying='y', side='right', showgrid=False),
        template="plotly_dark",
        height=800,
        hovermode="x unified",
        legend=dict(orientation="h", yanchor="bottom", y=1.02, xanchor="right", x=1)
    )

    return fig
