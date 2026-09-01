FROM postgres:18.6-bookworm

COPY --chmod=0755 deploy/postgres/entrypoint.sh /usr/local/bin/mail-agents-postgres-entrypoint

ENTRYPOINT ["/usr/local/bin/mail-agents-postgres-entrypoint"]
CMD ["postgres"]
