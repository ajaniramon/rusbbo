{% set title = locale.hk_resultados_de_la_encuesta %}
{% include "housekeeping/base/header.tpl" %}
{% autoescape 'html' %}
<div class="page-head">
  <div>{% if poll != null %}<h2 style="font-size:18px">{{ poll.getPollData().getQuestion() }}</h2>{% endif %}</div>
  <div class="actions"><a class="btn" href="{{ hk.path }}/infobus_polls">{{ locale.hk_volver_al_infobus|raw }}</a></div>
</div>
<div class="card">
  <div class="card-body">
    {% if noAnswers %}
      <div class="empty">{{ locale.hk_esta_encuesta_todavia_no_tiene_respuestas|raw }}</div>
    {% else %}
      <img src="{{ imageData }}" alt="{{ locale.hk_grafico_de_resultados|raw }}">
    {% endif %}
  </div>
</div>
{% endautoescape %}
{% include "housekeeping/base/footer.tpl" %}
