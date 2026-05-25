from configparser import ConfigParser
import os

CONFIG_FILE = 'config.ini'

def get_dune_api_key():
    config = ConfigParser()
    if os.path.exists(CONFIG_FILE):
        config.read(CONFIG_FILE)
        if 'DUNE' in config and 'api_key' in config['DUNE']:
            return config['DUNE']['api_key'].strip().strip('"').strip("'")
    return ""

def save_dune_api_key(api_key):
    config = ConfigParser()
    if os.path.exists(CONFIG_FILE):
        config.read(CONFIG_FILE)

    if 'DUNE' not in config:
        config.add_section('DUNE')

    config.set('DUNE', 'api_key', api_key)

    with open(CONFIG_FILE, 'w') as f:
        config.write(f)

def delete_dune_api_key():
    if not os.path.exists(CONFIG_FILE):
        return

    config = ConfigParser()
    config.read(CONFIG_FILE)

    if 'DUNE' in config and 'api_key' in config['DUNE']:
        config.remove_option('DUNE', 'api_key')
        with open(CONFIG_FILE, 'w') as f:
            config.write(f)
