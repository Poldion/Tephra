# Echter SSH-Server mit tmux für den Abschnitt „Sitzung halten“ des App-Tests (siehe app_test.dart):
#
#   docker build -t tephra-test-sshd -f app/integration_test/sshd.Dockerfile app/integration_test
#   docker run -d -p 127.0.0.1:2222:22 tephra-test-sshd
#   … --dart-define=TEPHRA_TEST_SSHD=127.0.0.1:2222:tester:geheim-tmux
FROM alpine:3.21
RUN apk add --no-cache openssh tmux bash \
 && ssh-keygen -A \
 && adduser -D -s /bin/bash tester \
 && echo 'tester:geheim-tmux' | chpasswd \
 && sed -i 's/^#\?PasswordAuthentication.*/PasswordAuthentication yes/' /etc/ssh/sshd_config
CMD ["/usr/sbin/sshd", "-D", "-e"]
