# Cross compile to Raspberry Pi. You need to have docker installed.
# See https://github.com/Ragnaroek/rust-on-raspberry-docker


# The version of rust we want to use
RUST_VERSION=1.36.1


# Location of the rust code
PROJECT=`pwd`

# Registry
CARGO=$HOME/.cargo/registry

DOCKER_IMAGE=ragnaroek/rust-raspberry:$RUST_VERSION

#####
# Setup dependencies
# https://github.com/Ragnaroek/rust-on-raspberry-docker#platform-dependencies-optional
###
# DEPS=`pwd`/rpi-deps
#
# [ -d $DEPS ] || mkdir $DEPS
#
# OPENSSL=openssl_1.1.1c-1_armhf.deb
# if [ ! -f $DEPS/$OPENSSL ] ; then
#   curl -o $DEPS/$OPENSSL http://ftp.debian.org/debian/pool/main/o/openssl/$OPENSSL
# fi

#####
# Pull and build
####

docker pull $DOCKER_IMAGE

docker run --volume $PROJECT:/home/cross/project --volume $CARGO:/home/cross/.cargo/registry \
  --volume $DEPS=/home/cross/deb-deps \
  $DOCKER_IMAGE \
  build --release

#####
# To debug the container you can use
#
# docker run -it --entrypoint /bin/bash ragnaroek/rust-raspberry:$RUST_VERSION
#
