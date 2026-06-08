import streamlit as st
import pandas as pd
import plotly.graph_objects as go
import requests
from config_manager import get_dune_api_key

# ===================== CONFIG =====================
PAIRS = ["BTC", "ETH", "SOL", "DOGE", "AVAX", "LINK"]
QUERY_ID = 3089944   # GMX V2 Long/Short

@st.cache_data(ttl=3600)
def get_long_short_data():
    api_key = get_dune_api_key()
    if not api_key:
        return pd.DataFrame()

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
            # Remove timezone for Plotly/Pandas compatibility
            if df['date'].dt.tz is not None:
                df['date'] = df['date'].dt.tz_localize(None)

        return df
    except Exception as e:
        st.error(f"Erreur lors de l'accès à Dune.com : {e}")
        return pd.DataFrame()

def get_long_short_whale_plot():
    """Fonction principale pour Streamlit"""
    col1, col2 = st.columns([1, 4])

    with col1:
        selected_pair = st.selectbox("Paire", PAIRS, index=0)

        mode = st.radio(
            "Mode d'affichage",
            ["Long vs Short", "Ratio Long/Short", "Open Interest Cumulé"],
            index=0
        )

        show_whales = st.checkbox("Afficher positions des gros wallets (Whales)", value=False)

    df_full = get_long_short_data()

    if df_full.empty:
        api_key = get_dune_api_key()
        if not api_key:
            st.warning("Clé API Dune manquante dans la configuration (Accueil).")
        else:
            st.warning("Aucune donnée retournée par Dune. Vérifiez votre Query ID ou vos paramètres.")
        return None

    # Filtrage par paire (Asset) - Recherche d'une colonne de symbole
    asset_col = next((c for c in df_full.columns if c.lower() in ['asset', 'symbol', 'pair', 'market']), None)
    if asset_col:
        df = df_full[df_full[asset_col].astype(str).str.upper().str.contains(selected_pair.upper())].copy()
    else:
        # Si pas de colonne asset, on suppose que le query est filtré ou on prend tout
        df = df_full.copy()

    if df.empty:
        st.warning(f"Aucune donnée trouvée pour la paire {selected_pair}.")
        return None

    # ===================== DÉTECTION COLONNES =====================
    # On cherche les colonnes qui contiennent 'long' et 'oi' ou 'position'
    long_col = next((c for c in df.columns if 'long' in c.lower() and ('oi' in c.lower() or 'position' in c.lower() or 'size' in c.lower())), None)
    short_col = next((c for c in df.columns if 'short' in c.lower() and ('oi' in c.lower() or 'position' in c.lower() or 'size' in c.lower())), None)

    if not long_col or not short_col:
        st.error("Colonnes Long/Short introuvables dans les données Dune.")
        st.write("Colonnes disponibles :", list(df.columns))
        return None

    # Conversion en numérique
    df[long_col] = pd.to_numeric(df[long_col], errors='coerce')
    df[short_col] = pd.to_numeric(df[short_col], errors='coerce')
    df = df.dropna(subset=[long_col, short_col])

    fig = go.Figure()
    title = ""

    # ===================== MODE 1: Long vs Short =====================
    if mode == "Long vs Short":
        fig.add_trace(go.Scatter(
            x=df['date'], y=df[long_col],
            name="Long OI", line=dict(color="#00C853", width=2.5)
        ))
        fig.add_trace(go.Scatter(
            x=df['date'], y=df[short_col],
            name="Short OI", line=dict(color="#FF1744", width=2.5)
        ))
        title = f"Long vs Short Open Interest - {selected_pair}"

    # ===================== MODE 2: Ratio =====================
    elif mode == "Ratio Long/Short":
        # Eviter division par zéro
        df['ratio'] = df[long_col] / df[short_col].replace(0, float('nan'))
        fig.add_trace(go.Scatter(
            x=df['date'], y=df['ratio'],
            name="Long/Short Ratio", line=dict(color="#FFD600", width=2.5)
        ))
        fig.add_hline(y=1.0, line_dash="dash", line_color="white", annotation_text="Équilibre (1.0)")
        title = f"Long/Short Ratio - {selected_pair}"

    # ===================== MODE 3: Open Interest Cumulé =====================
    else:
        df['cum_long'] = df[long_col].cumsum()
        df['cum_short'] = df[short_col].cumsum()

        fig.add_trace(go.Scatter(
            x=df['date'], y=df['cum_long'],
            name="Cumulative Long", line=dict(color="#00C853", width=2.5),
            fill='tozeroy', fillcolor='rgba(0,200,83,0.2)'
        ))
        fig.add_trace(go.Scatter(
            x=df['date'], y=df['cum_short'],
            name="Cumulative Short", line=dict(color="#FF1744", width=2.5),
            fill='tozeroy', fillcolor='rgba(255,23,68,0.2)'
        ))
        title = f"Open Interest Cumulé - {selected_pair}"

    fig.update_layout(
        title=title,
        height=650,
        template="plotly_dark",
        hovermode="x unified",
        legend=dict(orientation="h", yanchor="bottom", y=1.02, xanchor="right", x=1),
        margin=dict(t=60, b=100)
    )

    # ===================== SECTION WHALES =====================
    if show_whales:
        whale_long_col = next((c for c in df.columns if 'whale' in c.lower() and 'long' in c.lower()), None)
        whale_short_col = next((c for c in df.columns if 'whale' in c.lower() and 'short' in c.lower()), None)

        if whale_long_col and whale_short_col:
            df[whale_long_col] = pd.to_numeric(df[whale_long_col], errors='coerce')
            df[whale_short_col] = pd.to_numeric(df[whale_short_col], errors='coerce')

            fig.add_trace(go.Scatter(
                x=df['date'], y=df[whale_long_col],
                name="Whale Long", line=dict(color="#00C853", dash='dash', width=1.5)
            ))
            fig.add_trace(go.Scatter(
                x=df['date'], y=df[whale_short_col],
                name="Whale Short", line=dict(color="#FF1744", dash='dash', width=1.5)
            ))
        else:
            st.info("Données 'Whale' non disponibles pour cette paire dans le dataset actuel.")

    return fig

if __name__ == "__main__":
    # Test simple
    print("Module Long/Short Whale chargé (Requests version).")
