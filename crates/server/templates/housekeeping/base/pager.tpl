{% autoescape 'html' %}{% if pager != null %}
<div class="pager">
  <span>{% if pager.total == 0 %}{{ locale.hk_sin_resultados|raw }}{% else %}{{ pager.from }}&#8211;{{ pager.to }} {{ locale.hk_de|raw }} {{ pager.total }}{% endif %}</span>
  {% if pager.pages > 1 %}
  <div class="pages">
    {% if pager.hasPrev %}<a href="{{ pager.prevUrl }}">{{ locale.hk_anterior|raw }}</a>{% endif %}
    {% for n in pager.window %}
      {% if n == 0 %}<span class="gap">&#8230;</span>{% elseif n == pager.page %}<span class="on">{{ n }}</span>{% else %}<a href="{{ pager.url(n) }}">{{ n }}</a>{% endif %}
    {% endfor %}
    {% if pager.hasNext %}<a href="{{ pager.nextUrl }}">{{ locale.hk_siguiente|raw }}</a>{% endif %}
  </div>
  {% endif %}
</div>
{% endif %}{% endautoescape %}
