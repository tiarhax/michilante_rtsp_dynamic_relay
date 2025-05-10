FROM debian:bullseye

RUN apt-get update && apt-get install -y cron bash curl

COPY script.sh /usr/local/bin/script.sh
RUN chmod +x /usr/local/bin/script.sh \
    && touch /var/log/cron.log

RUN echo "* * * * * /usr/local/bin/script.sh >> /var/log/cron.log 2>&1" > /etc/cron.d/my-cron \
    && chmod 0644 /etc/cron.d/my-cron \
    && crontab /etc/cron.d/my-cron

# Run cron in foreground
CMD ["cron", "-f"]
