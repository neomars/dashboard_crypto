from configparser import ConfigParser
import os

CONFIG_FILE = 'config.ini'

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
