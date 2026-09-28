from configparser import ConfigParser
import os

# Chemin surchargable : l'application installée (paquet .deb) est en lecture
# seule sous /opt, son lanceur pointe donc vers ~/.config/dashboard-crypto/.
CONFIG_FILE = os.environ.get('DASHBOARD_CRYPTO_CONFIG', 'config.ini')

def get_api_key(section, option):
    config = ConfigParser()
    if os.path.exists(CONFIG_FILE):
        config.read(CONFIG_FILE)
        if section in config and option in config[section]:
            return config[section][option].strip().strip('"').strip("'")
    return ""

def save_api_key(section, option, api_key):
    config = ConfigParser()
    if os.path.exists(CONFIG_FILE):
        config.read(CONFIG_FILE)

    if section not in config:
        config.add_section(section)

    config.set(section, option, api_key)

    config_dir = os.path.dirname(CONFIG_FILE)
    if config_dir:
        os.makedirs(config_dir, exist_ok=True)
    with open(CONFIG_FILE, 'w') as f:
        config.write(f)

def delete_api_key(section, option):
    if not os.path.exists(CONFIG_FILE):
        return

    config = ConfigParser()
    config.read(CONFIG_FILE)

    if section in config and option in config[section]:
        config.remove_option(section, option)
        # Remove section if empty
        if not config.options(section):
            config.remove_section(section)

        with open(CONFIG_FILE, 'w') as f:
            config.write(f)

# Helper functions for Dune
def get_dune_api_key():
    return get_api_key('DUNE', 'api_key')

def save_dune_api_key(api_key):
    save_api_key('DUNE', 'api_key', api_key)

def delete_dune_api_key():
    delete_api_key('DUNE', 'api_key')

def get_dune_query_id(name, default):
    """Résout l'ID d'une requête Dune sans le coder en dur dans chaque module.

    Ordre de priorité : variable d'environnement DUNE_QUERY_<NAME> >
    section [DUNE_QUERIES] de config.ini > valeur par défaut.
    """
    env_value = os.environ.get(f"DUNE_QUERY_{name.upper()}")
    if env_value:
        return env_value

    ini_value = get_api_key('DUNE_QUERIES', name)
    return ini_value if ini_value else str(default)
