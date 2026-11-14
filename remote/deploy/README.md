# Linux中继部署：复用已有HTTPS服务

本包用于Linux x86_64，依赖glibc；最低版本、编译器和源码版本在包内`BUILD-INFO`，文件校验在`SHA256SUMS`。Agent继续在Mac运行，服务器只提供网页和密文转发。新增服务、域名和Caddy重载须在确定部署安排后执行。

## 首次安装

先确认已收到匹配的`bin/shifu-remote`、`web/`和`deploy/`，选好自己的子域名并将DNS指向服务器。共机沿用已有Caddy和80/443，不安装第二个HTTPS服务，不在这台小机编译Rust，不开放3843公网端口。

以下以`20261112-v1`为版本目录，替换为实际版本。先确认系统尚无同名服务用户和Shifu中继；已有安装按后文升级，不重复创建用户。

```sh
sudo useradd --system --user-group --home-dir /var/lib/shifu-remote --create-home --shell /usr/sbin/nologin shifu-remote
sudo install -d -o shifu-remote -g shifu-remote -m 700 /var/lib/shifu-remote
sudo install -d -m 755 /opt/shifu-remote/releases/20261112-v1
sudo tar -xzf /path/to/shifu-relay-linux-x86_64-VERSION.tar.gz -C /opt/shifu-remote/releases/20261112-v1
(cd /opt/shifu-remote/releases/20261112-v1 && sha256sum --check SHA256SUMS)
cat /opt/shifu-remote/releases/20261112-v1/BUILD-INFO
getconf GNU_LIBC_VERSION
/opt/shifu-remote/releases/20261112-v1/bin/shifu-remote relay help
sudo chown -R root:root /opt/shifu-remote/releases/20261112-v1
sudo ln -s /opt/shifu-remote/releases/20261112-v1 /opt/shifu-remote/current
sudo install -m 644 /opt/shifu-remote/current/deploy/shifu-relay.service /etc/systemd/system/shifu-relay.service
sudo systemd-analyze verify /etc/systemd/system/shifu-relay.service
sudo systemctl daemon-reload
sudo systemctl enable --now shifu-relay
sudo systemctl status shifu-relay --no-pager
curl --fail http://127.0.0.1:3843/
```

配置将运行资料固定到`/var/lib/shifu-remote`，不依赖服务用户的默认HOME。首次启动生成该目录中的`join-key`，权限仅服务用户可读。若本机HTTP不通，先看`sudo journalctl -u shifu-relay -n 50 --no-pager`，不先改现有房间或Caddy。

文件校验须全部通过，目标机glibc须达到`BUILD-INFO`要求，且程序帮助能运行，才继续建立`current`和启用服务。当前候选在Debian 12的amd64容器中测试；其他发行版须先核对，Alpine的musl环境不直接适用。`source_dirty=0`表示构建时源码工作区干净；正式候选同时保留对应提交和包指纹。

## 接到已有Caddy

备份当前`/etc/caddy/Caddyfile`，保留原房间的所有配置。只把`Caddyfile.shared.example`中的独立站点块加入现有文件，并把`shifu.example.com`换为实际域名，再执行：

```sh
sudo caddy validate --config /etc/caddy/Caddyfile --adapter caddyfile
sudo systemctl reload caddy
```

校验失败时不重载。重载后分别打开新域名和原有站点；已有长连接可能短暂断开，需要观察原服务是否恢复。新域名网页200只说明入口可达，后面还要由Mac登记、浏览器配对并实际接入终端。

## 把接入凭证交给Mac

通过既有IAP/SSH管理路径，将`/var/lib/shifu-remote/join-key`保存到本人Mac权限为600的文件。该文件由服务用户持有，管理者需通过已有sudo权限读取；不要改成全员可读，也不要把正文放进聊天、普通日志或发行包。

在Mac的Shifu「远程连接」中启用服务，填写自己的HTTPS地址和这份凭证，保存后配对随身浏览器。也可使用`shifu remote link HTTPS地址 --key-file 本地凭证文件`。Mac不需要开放入站端口。新加州Mac生成自己的主机身份，不复制测试Mac的设备密钥。

## 升级、停止和回退

升级前记下`readlink -f /opt/shifu-remote/current`的结果，保留该版本。把新包解压到新的`releases/<版本>`目录，先在新目录执行`sha256sum --check SHA256SUMS`，检查`BUILD-INFO`、glibc和`bin/shifu-remote relay help`，通过后再停止旧服务切换；不要覆盖`/var/lib/shifu-remote`或现有Caddy数据。

```sh
sudo systemctl stop shifu-relay
sudo ln -sfn /opt/shifu-remote/releases/NEW_VERSION /opt/shifu-remote/current
sudo systemctl start shifu-relay
sudo systemctl status shifu-relay --no-pager
```

重新打开网页，并确认Mac重新登记、浏览器接回同一测试会话；升级会断开网络连接，Agent在Mac上继续运行。服务文件有改动时，先重新安装和校验该文件，再执行`systemctl daemon-reload`。只有域名或代理配置变化才需要重载Caddy。

回退时停止中继，将`current`改回先前记录的旧版本目录，再启动；保留同一`join-key`和Mac配对资料，不清空运行目录。若需停用并取消服务器开机启动：

```sh
sudo systemctl disable --now shifu-relay
```

停用中继不结束Mac上的Agent。若撤去域名入口，只移除本模块新增的站点块，校验后重载，不用旧备份整份覆盖期间别人新增的配置。服务器凭证和现有TLS资料需要备份；它们不能代替Mac自己的主机及设备密钥备份。
