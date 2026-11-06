# The image only packages a prebuilt, static (musl) binary — nothing
# compiles here. The binary comes from `mise run build:linux`
# (scripts/build-linux.sh, what CI and the release workflow run too), which
# writes dist/woodmouse-linux-<arch>:
#
#   mise run build:docker
#   docker build .                       # BuildKit, buildx and podman set TARGETARCH
#   docker build --build-arg BINARY=dist/woodmouse-linux-arm64-dev .
#
# The runtime is Alpine, for a shell (`docker exec -it <container> sh`)
# and busybox tools when debugging in production. The binary itself is
# static and needs nothing from the base.
# An exact Alpine release, so the base changes only in a pull request
# (Dependabot's, as fix(deps): a release follows). `apk upgrade` takes the
# package fixes Alpine published since that release.
FROM alpine:3.24.4
ARG TARGETARCH
ARG BINARY=dist/woodmouse-linux-${TARGETARCH}
RUN apk upgrade --no-cache \
  && apk add --no-cache ca-certificates \
  && addgroup -S woodmouse && adduser -S -G woodmouse woodmouse \
  && mkdir /data && chown woodmouse /data
# --chmod: the executable bit does not survive every way a binary reaches dist/.
COPY --chmod=755 ${BINARY} /usr/local/bin/woodmouse
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
