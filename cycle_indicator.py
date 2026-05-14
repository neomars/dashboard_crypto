import pandas as pd
import numpy as np
import yfinance as yf
import plotly.graph_objects as go
from datetime import datetime
import streamlit as st

@st.cache_data
def get_cycle_plot():
    # 1. Fetch Data
    btc = yf.download("BTC-USD", start="2010-01-01")
    if btc.empty:
        return None

    if isinstance(btc.columns, pd.MultiIndex):
        close_prices = btc['Close']['BTC-USD']
    else:
        close_prices = btc['Close']

    df = pd.DataFrame({'Close': close_prices})
    df['LogPrice'] = np.log10(df['Close'])

    # 2. Angle calculation (1 rotation = 4 years)
    start_date = datetime(2009, 1, 1)
    df['Days'] = (df.index - start_date).days
    rotation_period = 4 * 365.25
    # Theta in degrees for Plotly. Direction is clockwise, 0 at North.
    df['Angle'] = (df['Days'] / rotation_period) * 360

    # 3. Days Post-Halving
    halvings = [
        datetime(2009, 1, 3),
        datetime(2012, 11, 28),
        datetime(2016, 7, 9),
        datetime(2020, 5, 11),
        datetime(2024, 4, 20)
    ]

    def get_days_post_halving(date):
        past_halvings = [h for h in halvings if h <= date]
        if not past_halvings:
            return 0
        last_halving = max(past_halvings)
        return (date - last_halving).days

    df['DaysPostHalving'] = df.index.map(get_days_post_halving)

    # 4. Z-Score / Color Proxy
    df['SMA200'] = df['Close'].rolling(window=200).mean()
    df['ZScore'] = (df['Close'] - df['SMA200']) / df['Close'].rolling(window=200).std()
    df['ZScore'] = df['ZScore'].fillna(0)
    # Inverse color scale: Green for low Z-score (Buy), Red for high (Sell)
    # Plotly's RdYlGn is Red-Yellow-Green. We want Green-Yellow-Red.
    df['ZScore_Clipped'] = np.clip(df['ZScore'], -2, 4)

    # 5. Create Plotly Figure
    fig = go.Figure()

    # Add background sectors for years
    # Q1: 0-90, Q2: 90-180, Q3: 180-270, Q4: 270-360
    # Adjusted colors for dark background
    colors = ['rgba(0, 255, 0, 0.03)', 'rgba(255, 255, 0, 0.03)',
              'rgba(255, 0, 0, 0.03)', 'rgba(0, 255, 255, 0.03)']

    for i in range(4):
        start_angle = i * 90
        end_angle = (i + 1) * 90
        theta = np.linspace(start_angle, end_angle, 50)
        fig.add_trace(go.Scatterpolar(
            r=[11]*len(theta) + [0],
            theta=list(theta) + [start_angle],
            fill='toself',
            fillcolor=colors[i],
            line=dict(color='rgba(0,0,0,0)'),
            hoverinfo='none',
            showlegend=False
        ))

    # Price levels (concentric circles)
    price_levels = [1, 10, 100, 1000, 10000, 100000, 1000000]
    for p in price_levels:
        r_val = np.log10(p)
        fig.add_trace(go.Scatterpolar(
            r=[r_val]*361,
            theta=list(range(361)),
            mode='lines',
            line=dict(color='rgba(100,100,100,0.1)', width=1),
            hoverinfo='none',
            showlegend=False
        ))
        fig.add_trace(go.Scatterpolar(
            r=[r_val],
            theta=[0],
            mode='text',
            text=[f"${p:,}"],
            textfont=dict(size=10, color="#888888"),
            hoverinfo='none',
            showlegend=False
        ))

    # Price Spiral
    fig.add_trace(go.Scatterpolar(
        r=df['LogPrice'],
        theta=df['Angle'],
        mode='markers',
        marker=dict(
            size=4,
            color=df['ZScore_Clipped'],
            colorscale='RdYlGn',
            reversescale=True,
            showscale=True,
            colorbar=dict(
                title="Sentiment (Z-Score)",
                thickness=15,
                x=1.05,
                tickvals=[-2, 1, 4],
                ticktext=['Froid', 'Neutre', 'Chaud']
            )
        ),
        text=[f"Date: {d.strftime('%Y-%m-%d')}<br>Prix: ${c:,.2f}<br>Jours post-halving: {h}"
              for d, c, h in zip(df.index, df['Close'], df['DaysPostHalving'])],
        hoverinfo='text',
        name='Prix BTC'
    ))

    # Add labels for years at the axes
    axes_labels = [
        (0, "2009, '13, '17, '21, '25"),
        (90, "2010, '14, '18, '22, '26"),
        (180, "2011, '15, '19, '23, '27"),
        (270, "2012, '16, '20, '24, '28")
    ]

    for angle, label in axes_labels:
        # Divider line
        fig.add_trace(go.Scatterpolar(
            r=[0, 11],
            theta=[angle, angle],
            mode='lines',
            line=dict(color='rgba(255,255,255,0.15)', width=1.5),
            hoverinfo='none',
            showlegend=False
        ))
        # Label
        fig.add_trace(go.Scatterpolar(
            r=[11.5],
            theta=[angle],
            mode='text',
            text=[label],
            textfont=dict(size=11, color="#00FFAA", family="serif"),
            hoverinfo='none',
            showlegend=False
        ))

    # Psych Phases
    phases = [
        ("BELIEF", 11.25), ("INTERMISSION", 33.75), ("THRILL", 56.25), ("EUPHORIA", 78.75),
        ("COMPLACENCY", 101.25), ("DENIAL", 123.75), ("PANIC", 146.25), ("CAPITULATION", 168.75),
        ("DEPRESSION", 191.25), ("ENDURING", 213.75), ("STAGNATION", 236.25), ("DISBELIEF", 258.75),
        ("DOUBT", 281.25), ("HOPE", 303.75), ("CALM", 326.25), ("OPTIMISM", 348.75),
    ]

    fig.add_trace(go.Scatterpolar(
        r=[9.5]*len(phases),
        theta=[p[1] for p in phases],
        mode='text',
        text=[p[0] for p in phases],
        textfont=dict(size=9, color="#AAAAAA", family="serif"),
        hoverinfo='none',
        showlegend=False
    ))

    # Year markers in quadrants
    fig.add_trace(go.Scatterpolar(
        r=[7.5, 7.5, 7.5, 7.5],
        theta=[45, 135, 225, 315],
        mode='text',
        text=["ANNÉE 1", "ANNÉE 2", "ANNÉE 3", "ANNÉE 4"],
        textfont=dict(size=24, color="rgba(255,255,255,0.05)", family="serif"),
        hoverinfo='none',
        showlegend=False
    ))

    fig.update_layout(
        template="plotly_dark",
        polar=dict(
            bgcolor='black',
            angularaxis=dict(
                direction='clockwise',
                period=360,
                visible=False,
                rotation=90 # Start at Top (North)
            ),
            radialaxis=dict(
                visible=False,
                range=[0, 12]
            )
        ),
        showlegend=False,
        paper_bgcolor='black',
        margin=dict(l=40, r=40, t=80, b=40),
        height=900,
        width=900,
        title=dict(
            text="BITCOIN 4-YEAR CYCLE - Hodler's Cheat Sheet",
            x=0.5,
            y=0.98,
            font=dict(size=24, family="serif", color="#00FFAA")
        )
    )

    return fig

if __name__ == "__main__":
    # Test script - requires kaleido for write_image
    try:
        fig = get_cycle_plot()
        if fig:
            fig.write_image("cycle_plotly_test.png")
            print("Plotly chart saved to cycle_plotly_test.png")
    except Exception as e:
        print(f"Could not save image (kaleido probably missing): {e}")
