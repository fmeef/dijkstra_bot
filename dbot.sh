#!/bin/sh


if ! which curl 2> /dev/null
then
  echo "curl needs to be installed, exiting"
  exit 1
fi

SUDO="sudo"

if ! which sudo 2> /dev/null && [ "$USER" != "root" ]
then
  echo "sudo not found, either run this script as root (yuck) or install sudo"
  exit 1
elif !which sudo 2> /dev/null
then
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


if which apt-get 2> /dev/null
then
  $SUDO apt-get update && $SUDO apt-get -y install podman podman-compose git
elif which dnf 2> /dev/null
then
  $SUDO dnf install -y podman podman-compose git
elif which pacman 2> /dev/null
then
  $SUDO pacman -S podman podman-compose git
elif which yum 2> /dev/null
then
  $SUDO yum -y install podman podman-compose git
else
  echo "No supported package manager found, exiting."
  exit 1
fi

if ! git clone --recursive https://github.com/fmeef/dijkstra_bot.git $HOME/.dijkstra 
then
  echo "Failed to clone git repository, make sure $HOME/.dijkstra is writable"
  exit 1
fi

if ! cd $HOME/.dijkstra 
then
  echo "$HOME/.dijkstra is not accessible"
  exit 1
fi

if ! podman-compose build
then
  echo "Failed to build dijkstra using podman. Your linux distro could be too old"
  echo "to support user namespaces, you could be running inside a proot container on a mobile device"
  echo "or you could be in a sandboxed environment of some kind. Try again with a modern computer"
  exit 1
fi


echo "Successfully installed dijkstra! To start first edit $HOME/.dijkstra/config/config.toml"
echo "then run"
echo "cd $HOME/.dijkstra && podman-compose up"



