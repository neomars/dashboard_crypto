import matplotlib.pyplot as plt
import numpy as np
import pandas as pd
import yfinance as yf
from datetime import datetime
import streamlit as st

def get_cycle_plot():
    # Style
    plt.rcParams['font.family'] = 'serif'
    bg_color = '#F5F5DC'  # Beige like the image
    text_color = '#2F4F4F' # Dark slate gray

    fig = plt.figure(figsize=(14, 14), facecolor=bg_color)
    ax = fig.add_subplot(111, projection='polar', facecolor=bg_color)

    # 1. Fetch Data
    # Get BTC data from yfinance (starts ~2014)
    btc = yf.download("BTC-USD", start="2010-01-01")

    # Check if we have data
    if btc.empty:
        return None

    # Handle MultiIndex if necessary (some versions of yfinance)
    if isinstance(btc.columns, pd.MultiIndex):
        close_prices = btc['Close']['BTC-USD']
    else:
        close_prices = btc['Close']

    df = pd.DataFrame({'Close': close_prices})
    df['LogPrice'] = np.log10(df['Close'])

    # 2. Angle calculation
    # 1 rotation = 4 years
    start_date = datetime(2009, 1, 1)
    df['Days'] = (df.index - start_date).days
    rotation_period = 4 * 365.25
    df['Angle'] = (df['Days'] / rotation_period) * 2 * np.pi

    # 3. Z-Score / Color Proxy
    # Using distance from 200-day moving average as a proxy for Z-score
    df['SMA200'] = df['Close'].rolling(window=200).mean()
    df['ZScore'] = (df['Close'] - df['SMA200']) / df['Close'].rolling(window=200).std()
    df['ZScore'] = df['ZScore'].fillna(0)

    # Clip Z-score for better color mapping
    z_min, z_max = -2, 4
    df['ZScore_Clipped'] = np.clip(df['ZScore'], z_min, z_max)

    # 4. Plotting the Price Spiral
    # Use scatter to color code by Z-score
    sc = ax.scatter(df['Angle'], df['LogPrice'], c=df['ZScore_Clipped'],
                    cmap='RdYlGn', s=5, alpha=0.6, edgecolors='none')

    # 5. Visualizing the structure
    # Circular Price Grids
    price_levels = [0.001, 0.01, 0.1, 1, 10, 100, 1000, 10000, 100000, 1000000]
    for p in price_levels:
        r = np.log10(p)
        ax.plot(np.linspace(0, 2*np.pi, 500), [r]*500, color='gray', linewidth=0.5, alpha=0.3)
        if p >= 0.1:
            label = f"${p:,.0f}" if p >= 1 else f"${p}"
            ax.text(0, r, label, color=text_color, fontsize=8, ha='center', va='bottom', alpha=0.7)

    # 6. Axes and Years
    ax.set_theta_zero_location('N')
    ax.set_theta_direction(-1) # Clockwise

    axes_labels = [
        (0, "2009, '13, '17, '21, '25"),
        (np.pi/2, "2010, '14, '18, '22, '26"),
        (np.pi, "2011, '15, '19, '23, '27"),
        (3*np.pi/2, "2012, '16, '20, '24, '28")
    ]

    for angle, label in axes_labels:
        ax.plot([angle, angle], [-3, 7], color='green', linewidth=1, alpha=0.5)
        ax.text(angle, 7.5, label, color=text_color, fontsize=10, ha='center', va='center', rotation=-np.degrees(angle))

    # 7. Phases Labels
    phases = [
        ("BELIEF", "TIME TO GET FULLY INVESTED.", 11.25),
        ("INTERMISSION", "THE TOP MIGHT BE IN.", 33.75),
        ("THRILL", "GOTTA TELL EVERYONE TO BUY.", 56.25),
        ("EUPHORIA", "I'M A GENIUS.", 78.75),

        ("COMPLACENCY", "WE JUST NEED TO COOL OFF\nFOR THE NEXT RALLY.", 101.25),
        ("DENIAL", "WHY AM I GETTING\nMARGIN CALLS?", 123.75),
        ("PANIC", "SHIT, EVERYONE IS SELLING.", 146.25),
        ("CAPITULATION", "I CAN'T AFFORD TO LOSE.", 168.75),

        ("DEPRESSION", "I'M AN IDIOT.", 191.25),
        ("ENDURING", "THIS IS GOING NOWHERE.", 213.75),
        ("STAGNATION", "THIS IS A SUCKER'S RALLY.", 236.25),
        ("DISBELIEF", "THIS RALLY WILL\nFAIL LIKE BEFORE.", 258.75),

        ("DOUBT", "IS THE HALVING PRICED IN?", 281.25),
        ("HOPE", "A RECOVERY IS POSSIBLE.", 303.75),
        ("CALM", "LET'S WAIT FOR CONFIRMATION.", 326.25),
        ("OPTIMISM", "THIS RALLY IS REAL.", 348.75),
    ]

    for title, subtitle, angle_deg in phases:
        angle_rad = np.radians(angle_deg)
        # Main label
        ax.text(angle_rad, 9.5, title, fontsize=14, fontweight='bold',
                color=text_color, ha='center', va='center', rotation=-angle_deg)
        # Subtitle
        ax.text(angle_rad, 8.5, subtitle, fontsize=8, color=text_color,
                ha='center', va='center', rotation=-angle_deg)

    # 8. Info Box (Top Right)
    info_text = (
        "BITCOIN 4-YEAR CYCLE\n\n"
        "Each rotation (circle) is a full market cycle\n"
        "of 4 years, starting at the top of the\n"
        "graph in 2009.\n\n"
        "To differentiate between bull and bear\n"
        "markets we use a momentum Z-Score\n"
        "as a proxy indicator."
    )
    fig.text(0.75, 0.85, info_text, fontsize=12, color=text_color,
             bbox=dict(facecolor='white', alpha=0.5, edgecolor='green'))

    # Legend for Z-Score
    cbar_ax = fig.add_axes([0.75, 0.72, 0.15, 0.02])
    cb = fig.colorbar(sc, cax=cbar_ax, orientation='horizontal')
    cb.set_ticks([])
    cbar_ax.text(0, -1.5, 'Value', transform=cbar_ax.transAxes, ha='left', fontsize=10)
    cbar_ax.text(1, -1.5, 'Overheated', transform=cbar_ax.transAxes, ha='right', fontsize=10)
    cbar_ax.set_title('Cycle Momentum (Z-Score)', fontsize=10, pad=5)

    # Clean up axes
    ax.set_yticklabels([])
    ax.set_xticklabels([])
    ax.spines['polar'].set_visible(False)
    ax.grid(False)
    ax.set_rmax(10)
    ax.set_rmin(-3)

    plt.tight_layout()
    return fig

if __name__ == "__main__":
    fig = get_cycle_plot()
    if fig:
        plt.savefig('cycle_test.png')
        print("Plot saved to cycle_test.png")
