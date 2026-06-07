import streamlit as st
import pandas as pd
import plotly.graph_objects as go
from dune_client.client import DuneClient
from config_manager import get_dune_api_key

# ===================== CONFIG =====================
PAIRS = ["BTC", "ETH", "SOL", "DOGE", "AVAX", "LINK"]
QUERY_ID = 3089944   # GMX V2 Long/Short

@st.cache_data(ttl=3600)
def get_long_short_data(asset: str):
    api_key = get_dune_api_key()
    if not api_key:
        return pd.DataFrame()

    dune = DuneClient(api_key)
    try:
        # Note: dune-client might require slightly different call depending on version,
        # but get_latest_result is standard.
        result = dune.get_latest_result(
            query_id=QUERY_ID,
            parameters=[{"name": "asset", "value": asset.upper(), "type": "text"}] if hasattr(dune, "run_query") else {"asset": asset.upper()}
        )
        # Handle different response structures if necessary
        rows = []
        if hasattr(result, 'result') and hasattr(result.result, 'rows'):
            rows = result.result.rows
        elif isinstance(result, list):
            rows = result
        else:
            # Fallback for different library versions
            try:
                rows = result.get_rows()
            except:
                rows = []

        df = pd.DataFrame(rows)

        if df.empty:
            return df

        # Detection des colonnes de date
        date_col = next((c for c in df.columns if c.lower() in ['date', 'block_time', 'time']), None)
        if date_col:
            df['date'] = pd.to_datetime(df[date_col])
            df = df.sort_values('date')

        return df
    except Exception as e:
        st.error(f"Erreur Dune : {e}")
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

    df = get_long_short_data(selected_pair)

    if df.empty:
        api_key = get_dune_api_key()
        if not api_key:
            st.warning("Clé API Dune manquante dans la configuration (Accueil).")
        else:
            st.warning("Aucune donnée retournée par Dune. Vérifiez votre Query ID ou vos paramètres.")
        return None

    # ===================== DÉTECTION COLONNES =====================
    long_col = next((c for c in df.columns if 'long' in c.lower() and ('oi' in c.lower() or 'position' in c.lower())), None)
    short_col = next((c for c in df.columns if 'short' in c.lower() and ('oi' in c.lower() or 'position' in c.lower())), None)

    if not long_col or not short_col:
        st.error("Colonnes Long/Short introuvables dans les données Dune.")
        st.write("Colonnes disponibles :", list(df.columns))
        return None

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
            # On ajoute ces traces au même graphique ou on pourrait en créer un autre.
            # L'utilisateur semble vouloir les ajouter si possible.
            # Mais attention à l'échelle. Pour l'instant, ajoutons les en pointillés.
            fig.add_trace(go.Scatter(
                x=df['date'], y=df[whale_long_col],
                name="Whale Long", line=dict(color="#00C853", dash='dash', width=1.5)
            ))
            fig.add_trace(go.Scatter(
                x=df['date'], y=df[whale_short_col],
                name="Whale Short", line=dict(color="#FF1744", dash='dash', width=1.5)
            ))
        else:
            st.info("Colonnes 'whale' non trouvées dans la query Dune.")

    return fig

if __name__ == "__main__":
    # Test simple
    print("Module Long/Short Whale chargé.")
