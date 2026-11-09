{% if article != null %}{% set title = locale.hk_editar_noticia ~ article.getTitle() %}{% else %}{% set title = locale.hk_editar_noticia_2 %}{% endif %}
{% include "housekeeping/base/header.tpl" %}
{% if article != null %}
{% set formAction = hk.path ~ '/articles/edit?id=' ~ article.getId() %}
{% include "housekeeping/articles_form.tpl" %}
{% else %}
<div class="empty">{{ locale.hk_esa_noticia_no_existe|raw }} <a href="{{ hk.path }}/articles">{{ locale.hk_volver_a_noticias|raw }}</a></div>
{% endif %}
{% include "housekeeping/base/footer.tpl" %}
