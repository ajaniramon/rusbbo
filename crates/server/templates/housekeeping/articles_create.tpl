{% set title = locale.hk_nueva_noticia_2 %}
{% include "housekeeping/base/header.tpl" %}
{% set formAction = hk.path ~ '/articles/create' %}
{% include "housekeeping/articles_form.tpl" %}
{% include "housekeeping/base/footer.tpl" %}
