#!/bin/sh

BOT_PREFIX=$HOME/.dijkstra

echo "Welcome to Dijkstra Bot, a modular and scalable group management bot written in rust"
echo ""
echo "This installer will automatically setup a containerized bot instance"
echo "\n"

if ! which curl >/dev/null 2>&1
then
  echo "curl needs to be installed, exiting"
  exit 1
fi

SUDO="sudo"

if ! which sudo >/dev/null 2>&1 && [ "$USER" != "root" ]
then
  echo "sudo not found, either run this script as root (yuck) or install sudo"
  exit 1
elif !which sudo >/dev/null 2>&1
then
  echo "warning! running as root. This is not recommended"
  SUDO=""
fi

if [ -d /data/data/com.termux ]
then
  echo "This installation script cannot be run under termux"
  echo "If someone on telegram or whatsapp told you to run this script you have been SCAMMED"
  echo "this project probably doesn't do what you think it does"
  echo "it definitely cannot hack anything and it won't add members to any chat groups"
  exit 1
fi

if [ -d $BOT_PREFIX ]
then
  echo "Dijkstra bot is already installed. Exiting."
  exit 1
fi

failpackages()
{
  echo "failed to install some packages, installation aborted"
  exit 1 
}

is_cry_ubuntu()
{
  if [ "$(lsb_release -rs)" = "22.04" ] 
  then
    return 0
  else
    return 1
  fi
}


setup_config()
{
  [ -f $BOX_PREFIX/config/config.toml ] && [ -f $BOT_PREFIX/db_pass.txt ] && return 0

  local db_pass="$(dd if=/dev/urandom bs=1 count=128 2>/dev/null | sha512sum | cut -d " " -f 1)"
  echo $db_pass > $BOT_PREFIX/db_pass.txt 
  local bot_token=""
  read -p "Enter bot token from @BotFather > " bot_token < /dev/tty

  [ -z "$bot_token" ] && echo "Bot token must not be empty" && return 1

  cat <<EOF > $BOT_PREFIX/config/config.toml
bot_token = '$bot_token'
[persistence]
database_connection = 'postgresql://bobot:$db_pass@db/bobot'
redis_connection = 'redis://redis'

[webhook]
enable_webhook = false
webhook_url = 'https://bot.ustc.edu.cn'
listen = '0.0.0.0:8080'

[logging]
log_level = 'info'
prometheus_hook = '0.0.0.0:9999'

[timing]
cache_timeout = 172800
antifloodwait_count = 10
antifloodwait_time = 20
ignore_chat_time = 300

[admin]
sudo_users = []
support_users = []
EOF
return 0
}


setup_systemd()
{
  if ! mkdir -p $HOME/.config/systemd/user >/dev/null 2>&1
  then
    echo "Failed to create .config/systemd/user directory"
    exit 1
  fi

  if [ -f $HOME/.config/systemd/user/bot.service ]
  then
    echo ".config/systemd/user/bot.service already exists"
    exit 1
  fi

  cat << EOF > $HOME/.config/systemd/user/bot.service
[Unit]
Description=Run telegram bot

[Service]
Restart=always
ExecStart=podman-compose -f $BOT_PREFIX/docker-compose.yml up
ExecStop=podman-compose -f $BOT_PREFIX/docker-compose.yml down
Type=simple

[Install]
WantedBy=default.target
EOF

  if ! systemctl --user daemon-reload
  then
    echo "failed to reload systemd"
    exit 1
  fi
  
  if ! systemctl --user enable bot.service >/dev/null 2>&1  
  then
    echo "failed to enable bot via systemd"
    exit 1
  fi
}


echo "installing packages..."
if which apt-get >/dev/null 2>&1
then
  export DEBIAN_FRONTEND=noninteractive
  if is_cry_ubuntu
  then
    curl http://archive.ubuntu.com/ubuntu/pool/universe/g/golang-github-containernetworking-plugins/containernetworking-plugins_1.1.1+ds1-1_amd64.deb > /tmp/cry.deb 2> /dev/null
    $SUDO dpkg -i /tmp/cry.deb >/dev/null 2>&1 || failpackages
  fi  
  $SUDO apt-get update >/dev/null 2>&1 && $SUDO apt-get -y install podman git python3-pip >/dev/null 2>&1 || failpackages
  $SUDO pip3 install podman-compose >/dev/null 2>&1 || failpackages
elif which dnf >/dev/null 2>&1
then
  $SUDO dnf install -y podman podman-compose git containernetworking-plugins >/dev/null 2>&1
elif which pacman >/dev/null 2>&1  
then
  $SUDO pacman -S podman podman-compose git cni-plugins >/dev/null 2>&1
elif which yum >/dev/null 2>&1
then
  $SUDO yum -y install podman podman-compose git containernetworking-plugins >/dev/null 2>&1
else
  echo "No supported package manager found, exiting."
  exit 1
fi

echo "Cloning git repository"

if ! git clone --recursive https://github.com/fmeef/dijkstra_bot.git $BOT_PREFIX >/dev/null 2>&1
then
  echo "Failed to clone git repository, make sure $BOT_PREFIX is writable"
  exit 1
fi

if ! cd $BOT_PREFIX
then
  echo "$BOT_PREFIX is not accessible"
  exit 1
fi

if ! setup_config
then
  echo "Failed to setup bot config. make sure you entered your token correctly"
  exit 1
fi

setup_systemd

echo "Successfully installed dijkstra! To start first edit $BOT_PREFIX/config/config.toml"
echo "then run"
echo "cd $BOT_PREFIX && podman-compose up"



