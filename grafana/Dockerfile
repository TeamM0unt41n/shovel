# Copyright (C) 2025-2026  A. Iooss
# SPDX-License-Identifier: CC0-1.0
FROM grafana/grafana-oss:12.4.2

# Setup provisioning
ENV GF_LOG_LEVEL=warn \
    GF_ANALYTICS_CHECK_FOR_PLUGIN_UPDATES=false \
    GF_ANALYTICS_CHECK_FOR_UPDATES=false \
    GF_ANALYTICS_REPORTING_ENABLED=false \
    GF_AUTH_ANONYMOUS_ENABLED=true \
    GF_AUTH_ANONYMOUS_HIDE_VERSION=true \
    GF_DASHBOARDS_DEFAULT_HOME_DASHBOARD_PATH=/var/lib/grafana/dashboards/home.json
COPY ./provisioning /etc/grafana/provisioning
COPY ./dashboards /dashboards_template
COPY ./entrypoint.sh /entrypoint.sh
ENTRYPOINT ["/entrypoint.sh"]
