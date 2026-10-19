# The image only packages a prebuilt, static (musl) binary — nothing
# compiles here. The binary comes from `mise run build:backend` (what CI
# and the release workflow run too) through the `bin` build context:
#
#   mise run build:docker
#   docker build --build-context bin=<dir holding "woodmouse"> .
#
# The runtime is Alpine, for a shell (`docker exec -it <container> sh`)
# and busybox tools when debugging in production. The binary itself is
# static and needs nothing from the base.
FROM alpine:3
RUN addgroup -S woodmouse && adduser -S -G woodmouse woodmouse \
  && mkdir /data && chown woodmouse /data
COPY --from=bin /woodmouse /usr/local/bin/woodmouse
ENV PORT=8000 \
    DATABASE_URL=sqlite:/data/woodmouse.db \
    DATA_DIR=/data
VOLUME /data
EXPOSE 8000

# `woodmouse healthcheck` asks the running server's /api/health.
HEALTHCHECK --interval=10s --timeout=3s --start-period=5s --retries=3 \
  CMD ["/usr/local/bin/woodmouse", "healthcheck"]

USER woodmouse
ENTRYPOINT ["/usr/local/bin/woodmouse"]
