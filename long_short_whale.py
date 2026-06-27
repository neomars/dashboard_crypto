import streamlit as st
import pandas as pd
import plotly.graph_objects as go
import requests
import yfinance as yf
from config_manager import get_dune_api_key

# ===================== CONFIG =====================
PAIRS = ["BTC", "ETH", "SOL", "DOGE", "AVAX", "LINK"]
QUERY_ID = 3089944   # GMX V2 Long/Short

@st.cache_data(ttl=3600)
def get_long_short_data():
    """Récupère les données brutes depuis Dune Analytics"""
    api_key = get_dune_api_key()
    if not api_key:
        return pd.DataFrame()

    # On récupère les derniers résultats de la query
    url = f"https://api.dune.com/api/v1/query/{QUERY_ID}/results"
    headers = {
        "X-Dune-API-Key": api_key,
        "Accept-Encoding": "identity"
    }

    try:
        response = requests.get(url, headers=headers, timeout=20)
        if response.status_code != 200:
            st.error(f"Erreur API Dune : {response.status_code} - {response.text}")
            return pd.DataFrame()

        data = response.json().get('result', {}).get('rows', [])
        df = pd.DataFrame(data)

        if df.empty:
            return df

        # Detection des colonnes de date
        date_col = next((c for c in df.columns if c.lower() in ['date', 'block_time', 'time']), None)
        if date_col:
            df['date'] = pd.to_datetime(df[date_col])
            df = df.sort_values('date')
            if df['date'].dt.tz is not None:
                df['date'] = df['date'].dt.tz_localize(None)

        return df
    except Exception as e:
        st.error(f"Erreur lors de l'accès à Dune.com : {e}")
        return pd.DataFrame()

def get_long_short_plot():
    """Fonction principale pour Streamlit"""
    col1, col2 = st.columns([1, 4])

    with col1:
        selected_pair = st.selectbox("Paire", PAIRS, index=0)

        mode = st.radio(
            "Mode d'affichage",
            ["Long vs Short", "Ratio Long/Short", "Open Interest Cumulé"],
            index=0
        )

    # Récupération des données (cachées par st.cache_data)
    df_full = get_long_short_data()

    if df_full.empty:
        api_key = get_dune_api_key()
        if not api_key:
            st.warning("Clé API Dune manquante dans la configuration (Accueil).")
        else:
            st.warning("Aucune donnée retournée par Dune. Vérifiez votre Query ID ou vos paramètres.")
        return None

    # Filtrage par paire (Asset)
    # On cherche une colonne qui identifie l'asset (ex: 'market_symbol', 'symbol', 'asset')
    asset_col = next((c for c in df_full.columns if any(sub in c.lower() for sub in ['asset', 'symbol', 'pair', 'market', 'token'])), None)

    if asset_col:
        # On filtre les lignes correspondant à la sélection
        df = df_full[df_full[asset_col].astype(str).str.upper().str.contains(selected_pair.upper())].copy()
    else:
        # Fallback si pas de colonne asset, on prend tout (le query est peut-être déjà filtré par défaut sur BTC)
        df = df_full.copy()

    if df.empty:
        st.warning(f"Aucune donnée trouvée pour la paire {selected_pair} dans les résultats Dune.")
        return None

    # ===================== DÉTECTION COLONNES POSITION =====================
    # On cherche les colonnes Long OI et Short OI
    long_col = next((c for c in df.columns if 'long' in c.lower() and ('oi' in c.lower() or 'size' in c.lower() or 'position' in c.lower())), None)
    short_col = next((c for c in df.columns if 'short' in c.lower() and ('oi' in c.lower() or 'size' in c.lower() or 'position' in c.lower())), None)

    if not long_col or not short_col:
        st.error("Colonnes Long/Short introuvables. Vérifiez la structure des données Dune.")
        st.write("Colonnes disponibles :", list(df.columns))
        return None

    # Conversion en numérique pour calculs et affichage
    df[long_col] = pd.to_numeric(df[long_col], errors='coerce')
    df[short_col] = pd.to_numeric(df[short_col], errors='coerce')
    df = df.dropna(subset=[long_col, short_col, 'date'])

    # ===================== PRIX DE L'ASSET (YFINANCE) =====================
    ticker = f"{selected_pair}-USD"
    min_date = df['date'].min().strftime('%Y-%m-%d')
    price_data = yf.download(ticker, start=min_date, progress=False)

    if not price_data.empty:
        if isinstance(price_data.columns, pd.MultiIndex):
            price_close = price_data['Close'][ticker]
        else:
            price_close = price_data['Close']

        price_df = pd.DataFrame({'price_date': price_data.index, 'Asset_Price': price_close.values})
        price_df['price_date'] = pd.to_datetime(price_df['price_date']).dt.tz_localize(None)

        # Normalisation pour le merge (date-only)
        df['date_only'] = df['date'].dt.normalize()
        price_df['price_date_only'] = price_df['price_date'].dt.normalize()
        price_df = price_df.drop_duplicates('price_date_only')

        df = pd.merge(df, price_df[['price_date_only', 'Asset_Price']],
                     left_on='date_only', right_on='price_date_only', how='left')
        df['Asset_Price'] = df['Asset_Price'].ffill()

    # ===================== GRAPHIQUE =====================
    fig = go.Figure()
    title = f"{selected_pair} - Positions Ouvertes (GMX V2)"

    # Mode 1: Long vs Short
    if mode == "Long vs Short":
        fig.add_trace(go.Scatter(
            x=df['date'], y=df[long_col],
            name=f"Long OI ({long_col})", line=dict(color="#00C853", width=2.5)
        ))
        fig.add_trace(go.Scatter(
            x=df['date'], y=df[short_col],
            name=f"Short OI ({short_col})", line=dict(color="#FF1744", width=2.5)
        ))

    # Mode 2: Ratio
    elif mode == "Ratio Long/Short":
        df['ratio'] = df[long_col] / df[short_col].replace(0, float('nan'))
        fig.add_trace(go.Scatter(
            x=df['date'], y=df['ratio'],
            name="Ratio Long/Short", line=dict(color="#FFD600", width=2.5)
        ))
        fig.add_hline(y=1.0, line_dash="dash", line_color="white", annotation_text="Équilibre")
        title = f"{selected_pair} - Ratio Long/Short"

    # Mode 3: Open Interest Cumulé
    else:
        df['cum_long'] = df[long_col].cumsum()
        df['cum_short'] = df[short_col].cumsum()

        fig.add_trace(go.Scatter(
            x=df['date'], y=df['cum_long'],
            name="Cumulative Long", line=dict(color="#00C853", width=2.5),
            fill='tozeroy', fillcolor='rgba(0,200,83,0.1)'
        ))
        fig.add_trace(go.Scatter(
            x=df['date'], y=df['cum_short'],
            name="Cumulative Short", line=dict(color="#FF1744", width=2.5),
            fill='tozeroy', fillcolor='rgba(255,23,68,0.1)'
        ))
        title = f"{selected_pair} - Volume OI Cumulé"

    # Superposition du prix
    if 'Asset_Price' in df.columns:
        fig.add_trace(go.Scatter(
            x=df['date'], y=df['Asset_Price'],
            name=f"Prix {selected_pair} (USD)",
            line=dict(color='#00CCFF', width=1.5, dash='dot'),
            yaxis="y2"
        ))

    fig.update_layout(
        title=title,
        height=650,
        template="plotly_dark",
        hovermode="x unified",
        legend=dict(orientation="h", yanchor="bottom", y=1.02, xanchor="right", x=1),
        yaxis=dict(title="Open Interest / Ratio", side="left"),
        yaxis2=dict(
            title=f"Prix {selected_pair} (USD)",
            overlaying="y",
            side="right",
            showgrid=False
        ),
        margin=dict(t=80, b=100)
    )

    return fig

if __name__ == "__main__":
    print("Module Long/Short Whale mis à jour (Whales supprimées, switching asset corrigé).")
