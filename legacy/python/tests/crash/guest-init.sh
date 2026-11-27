#!/tt/busybox sh
set -eu
export PATH=/tt/bin:/usr/bin
export PYTHONHOME=/usr
export PYTHONDONTWRITEBYTECODE=1
mount -t devtmpfs devtmpfs /dev
mount -t proc proc /proc
mount -t sysfs sysfs /sys
while read -r mod; do insmod "$mod"; done < /tt/modules/order
i=0
while [ ! -b /dev/vda ]; do
    i=$((i+1))
    if [ "$i" -ge 30 ]; then echo 'TT_FAIL no virtual disk'; exit 1; fi
    sleep 1
done
mount -t ext4 -o rw /dev/vda /mnt/data
export HOME=/mnt/data/home
export XDG_STATE_HOME=/mnt/data/state
export XDG_CONFIG_HOME=/mnt/data/config
export XDG_DATA_HOME=/mnt/data/share
/usr/bin/python3 /tt/guest.py || echo 'TT_FAIL guest exited unsuccessfully'
# Never shut down gracefully during a cut scenario.
while :; do sleep 60; done
